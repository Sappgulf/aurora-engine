//! Application trait and cross-platform runner (native + WASM).

use glam::Vec2;
use std::fmt;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::assets::AssetManifest;
use crate::audio::Audio;
use crate::color::Color;
#[cfg(not(target_arch = "wasm32"))]
use crate::devtools::DebugHarness;
use crate::diagnostics::{DiagnosticSnapshot, Diagnostics};
use crate::input::{GamepadFrame, Input, PadButton};
use crate::loader::AssetLoadQueue;
use crate::performance::QualityController;
use crate::platform::{LifecycleEvent, LifecycleState, SurfaceStatus};
use crate::profile::EngineProfile;
use crate::renderer::Renderer;
use crate::save::{SaveEnvelope, SaveSource, SaveStore};
use crate::time::Time;
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;

/// Per-frame context passed into game callbacks.
pub struct FrameCtx<'a> {
    /// Timing (delta, fixed step, elapsed).
    pub time: &'a mut Time,
    /// Keyboard / mouse state for this frame.
    pub input: &'a Input,
    /// GPU renderer + camera + draw queue.
    pub renderer: &'a mut Renderer,
    /// Procedural beeps / SFX.
    pub audio: &'a mut Audio,
    /// Runtime diagnostics snapshot source for HUDs and telemetry.
    pub diagnostics: &'a Diagnostics,
    /// Engine-owned settings profile shared by the game and platform shell.
    pub profile: &'a mut EngineProfile,
}

/// Implement this for your game / demo.
pub trait Game: 'static {
    fn name(&self) -> &str {
        "Aurora Engine"
    }

    /// Called once after the GPU renderer is ready.
    fn on_start(&mut self, _renderer: &mut Renderer) {}

    /// Whether the presentation layer may adapt quality from frame headroom.
    fn adaptive_quality(&self) -> bool {
        true
    }

    /// Gives the game one chance to migrate legacy settings into the engine
    /// profile after the game's own save data has been loaded.
    fn on_profile_loaded(&mut self, _profile: &mut EngineProfile, _is_new: bool) {}

    /// Optional lifecycle notification from the platform shell.
    fn on_lifecycle(&mut self, _event: LifecycleEvent) {}

    /// Fixed-timestep simulation (default 60 Hz). Optional.
    fn on_fixed_update(&mut self, _ctx: &mut FrameCtx<'_>) {}

    /// Called every frame before render (variable delta).
    fn on_update(&mut self, ctx: &mut FrameCtx<'_>);

    /// Optional post-update hook for deterministic render prep after all
    /// simulation work has completed for this frame.
    fn on_post_update(&mut self, _ctx: &mut FrameCtx<'_>) {}

    /// Optional authoritative asset manifest for runtime loading diagnostics.
    /// Synchronous games can report entries as ready after `on_start`; games
    /// without a manifest retain the empty-queue behavior.
    fn asset_manifest(&self) -> Option<AssetManifest> {
        None
    }

    /// Optional: handle raw window events after engine input is updated.
    /// Return `true` if the event was consumed.
    fn on_event(&mut self, _event: &WindowEvent) -> bool {
        false
    }

    /// Game state published to agent tooling (see [`crate::agent`]). Return
    /// `None` to publish nothing; whatever you return is what `{"cmd":
    /// "state"}` requests and `window.auroraState()` (web) hand back.
    fn agent_state(&self) -> Option<serde_json::Value> {
        None
    }

    /// Game-specific agent actions (`{"cmd":"game","action":"...","args":{..}}`).
    /// Return the reply payload, or `None` to report "unsupported action".
    fn on_agent_command(
        &mut self,
        _action: &str,
        _args: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        None
    }
}

enum UserEvent {
    RendererReady(Renderer),
}

const PROFILE_SAVE_VERSION: u32 = 1;

/// Launch a game on the current platform (desktop window or browser canvas).
pub fn run<G: Game>(game: G) {
    if let Err(error) = run_result(game) {
        log::error!("Aurora failed to start: {error}");
    }
}

#[derive(Debug)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum EngineStartError {
    EventLoopBuild(String),
    EventLoopRun(String),
    WindowCreate(String),
}

impl fmt::Display for EngineStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLoopBuild(error) => write!(f, "failed to create event loop: {error}"),
            Self::EventLoopRun(error) => write!(f, "event loop terminated with error: {error}"),
            Self::WindowCreate(error) => write!(f, "failed to create window: {error}"),
        }
    }
}

impl std::error::Error for EngineStartError {}

fn run_result<G: Game>(game: G) -> Result<(), EngineStartError> {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            init_logging_wasm();
        } else {
            init_logging_native();
        }
    }

    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|error| EngineStartError::EventLoopBuild(error.to_string()))?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let proxy = event_loop.create_proxy();

    let profile_store = SaveStore::<EngineProfile>::new("aurora-engine", game.name());
    let mut diagnostics = Diagnostics::default();
    let (profile, profile_is_new) = match profile_store.load_with_source() {
        Ok(Some(loaded)) if loaded.envelope.format_version <= PROFILE_SAVE_VERSION => {
            if loaded.source == SaveSource::Backup {
                diagnostics.record_save_recovery();
            }
            (loaded.envelope.payload.normalized(), false)
        }
        Ok(Some(loaded)) => {
            log::warn!(
                "ignoring unsupported engine profile format {} (supported {})",
                loaded.envelope.format_version,
                PROFILE_SAVE_VERSION
            );
            diagnostics.record_save_failure();
            (EngineProfile::default(), true)
        }
        Ok(None) => (EngineProfile::default(), true),
        Err(error) => {
            log::warn!("could not load engine profile: {error}");
            diagnostics.record_save_failure();
            (EngineProfile::default(), true)
        }
    };
    let mut input = Input::new();
    let mut audio = Audio::new();
    profile.apply_input_audio(&mut input, &mut audio);

    let asset_load_queue = game
        .asset_manifest()
        .map_or_else(AssetLoadQueue::default, |manifest| {
            AssetLoadQueue::from_manifest(&manifest)
        });
    let app = EngineApp {
        game: Some(game),
        window: None,
        renderer: None,
        time: Time::new(),
        input,
        diagnostics,
        asset_load_queue,
        audio,
        lifecycle: LifecycleState::default(),
        quality_controller: QualityController::new(16.67),
        profile,
        profile_store,
        profile_is_new,
        proxy,
        init_started: false,
        #[cfg(not(target_arch = "wasm32"))]
        devtools: DebugHarness::from_env(),
        #[cfg(not(target_arch = "wasm32"))]
        agent_server: crate::agent::AgentServer::from_env(),
        #[cfg(not(target_arch = "wasm32"))]
        pads: PadBackend::new(),
    };

    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            use winit::platform::web::EventLoopExtWebSys;
            event_loop.spawn_app(app);
        } else {
            let mut app = app;
            event_loop
                .run_app(&mut app)
                .map_err(|error| EngineStartError::EventLoopRun(error.to_string()))?;
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn init_logging_native() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
}

#[cfg(target_arch = "wasm32")]
fn init_logging_wasm() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    let _ = console_log::init_with_level(log::Level::Info);
}

#[cfg(target_arch = "wasm32")]
fn web_surface_size(
    window: &Window,
) -> (winit::dpi::PhysicalSize<u32>, winit::dpi::PhysicalSize<u32>) {
    use winit::platform::web::WindowExtWebSys;

    let Some(canvas) = window.canvas() else {
        let size = window.inner_size();
        return (size, size);
    };
    let scale = web_sys::window()
        .map(|browser| browser.device_pixel_ratio())
        .unwrap_or(1.0);
    let logical_width = canvas.client_width().max(1) as u32;
    let logical_height = canvas.client_height().max(1) as u32;
    let width = (logical_width as f64 * scale).round() as u32;
    let height = (logical_height as f64 * scale).round() as u32;
    canvas.set_width(width);
    canvas.set_height(height);
    (
        winit::dpi::PhysicalSize::new(width, height),
        winit::dpi::PhysicalSize::new(logical_width, logical_height),
    )
}

struct EngineApp<G: Game> {
    game: Option<G>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    time: Time,
    input: Input,
    diagnostics: Diagnostics,
    asset_load_queue: AssetLoadQueue,
    audio: Audio,
    lifecycle: LifecycleState,
    quality_controller: QualityController,
    profile: EngineProfile,
    profile_store: SaveStore<EngineProfile>,
    profile_is_new: bool,
    proxy: EventLoopProxy<UserEvent>,
    init_started: bool,
    #[cfg(not(target_arch = "wasm32"))]
    devtools: Option<DebugHarness>,
    #[cfg(not(target_arch = "wasm32"))]
    agent_server: Option<crate::agent::AgentServer>,
    #[cfg(not(target_arch = "wasm32"))]
    pads: PadBackend,
}

/// Native gamepad backend: one gilrs context plus stable slot assignment.
#[cfg(not(target_arch = "wasm32"))]
struct PadBackend {
    ctx: Option<gilrs::Gilrs>,
    slots: HashMap<gilrs::GamepadId, usize>,
    next_slot: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl PadBackend {
    fn new() -> Self {
        let ctx = match gilrs::Gilrs::new() {
            Ok(ctx) => Some(ctx),
            Err(error) => {
                log::info!("gamepads unavailable: {error}");
                None
            }
        };
        Self {
            ctx,
            slots: HashMap::new(),
            next_slot: 0,
        }
    }

    /// Re-reads every connected pad and pushes snapshots into `input`.
    /// Full-resync per frame keeps state truthful across hot-plugs without
    /// depending on event queue subtleties.
    fn poll(&mut self, input: &mut Input) {
        use gilrs::{Axis, Button};
        let Some(ctx) = self.ctx.as_mut() else {
            return;
        };
        // Drain events so internal filters stay current (connect/disconnect).
        while ctx.next_event().is_some() {}

        let mut seen_ids: std::collections::HashSet<gilrs::GamepadId> =
            std::collections::HashSet::new();
        let mut used_slots = [false; crate::input::MAX_GAMEPADS];
        for (id, gamepad) in ctx.gamepads() {
            seen_ids.insert(id);
            let existing_slot = self.slots.get(&id).copied();
            let mut slot = existing_slot.unwrap_or_else(|| {
                let slot = self.next_slot % crate::input::MAX_GAMEPADS;
                self.next_slot += 1;
                slot
            });
            if used_slots[slot] {
                if let Some(free_slot) =
                    (0..crate::input::MAX_GAMEPADS).find(|candidate| !used_slots[*candidate])
                {
                    slot = free_slot;
                } else {
                    // When more than MAX_GAMEPADS are connected this frame,
                    // skip new ownership to avoid slot stomping.
                    if existing_slot.is_some() {
                        seen_ids.remove(&id);
                    }
                    continue;
                }
            }
            if existing_slot != Some(slot) {
                self.slots.insert(id, slot);
            }
            used_slots[slot] = true;

            let mut buttons = [false; 16];
            for (pad_button, aurora_button) in [
                (Button::South, PadButton::South),
                (Button::East, PadButton::East),
                (Button::West, PadButton::West),
                (Button::North, PadButton::North),
                (Button::LeftTrigger, PadButton::LeftShoulder),
                (Button::RightTrigger, PadButton::RightShoulder),
                (Button::Select, PadButton::Back),
                (Button::Start, PadButton::Start),
                (Button::DPadUp, PadButton::DpadUp),
                (Button::DPadDown, PadButton::DpadDown),
                (Button::DPadLeft, PadButton::DpadLeft),
                (Button::DPadRight, PadButton::DpadRight),
            ] {
                if gamepad.is_pressed(pad_button) {
                    buttons[aurora_button.index().min(15)] = true;
                }
            }

            let axis = |axis: Axis| -> f32 {
                gamepad
                    .axis_data(axis)
                    .map(|data| data.value())
                    .unwrap_or(0.0)
            };
            let frame = GamepadFrame {
                connected: true,
                buttons,
                left_stick: Vec2::new(axis(Axis::LeftStickX), -axis(Axis::LeftStickY)),
                right_stick: Vec2::new(axis(Axis::RightStickX), -axis(Axis::RightStickY)),
                triggers: axis(Axis::LeftZ).max(axis(Axis::RightZ)).max(0.0),
            };
            input.push_gamepad_frame(slot, &frame);
        }

        let stale = self
            .slots
            .iter()
            .filter_map(|(id, index)| (!seen_ids.contains(id)).then_some((*id, *index)))
            .collect::<Vec<_>>();
        for (_, index) in &stale {
            input.push_gamepad_frame(
                *index,
                &GamepadFrame {
                    connected: false,
                    ..Default::default()
                },
            );
        }
        for (id, _) in stale {
            self.slots.remove(&id);
        }
    }

    /// Plays queued force-feedback impulses through gilrs. Pads without FF
    /// support (or unknown slots) are skipped without fuss.
    fn apply_rumbles(&mut self, input: &mut Input) {
        let Some(ctx) = self.ctx.as_mut() else {
            return;
        };
        for request in input.drain_rumbles() {
            let Some((&id, _)) = self.slots.iter().find(|(_, slot)| **slot == request.slot) else {
                continue;
            };
            let Some(gamepad) = ctx.connected_gamepad(id) else {
                continue;
            };
            let play_for = gilrs::ff::Ticks::from_ms((request.duration * 1000.0) as u32);
            let scheduling = gilrs::ff::Replay {
                after: gilrs::ff::Ticks::from_ms(0),
                play_for,
                with_delay: gilrs::ff::Ticks::from_ms(0),
            };
            let mut builder = gilrs::ff::EffectBuilder::new();
            builder.add_effect(gilrs::ff::BaseEffect {
                kind: gilrs::ff::BaseEffectType::Strong {
                    magnitude: (request.low * u16::MAX as f32) as u16,
                },
                scheduling,
                envelope: gilrs::ff::Envelope::default(),
            });
            builder.add_effect(gilrs::ff::BaseEffect {
                kind: gilrs::ff::BaseEffectType::Weak {
                    magnitude: (request.high * u16::MAX as f32) as u16,
                },
                scheduling,
                envelope: gilrs::ff::Envelope::default(),
            });
            builder.add_gamepad(&gamepad);
            if let Ok(effect) = builder.finish(ctx) {
                let _ = effect.play();
            }
        }
    }
}

fn record_lifecycle_transition(diagnostics: &mut Diagnostics, lifecycle: &LifecycleState) {
    diagnostics.record_lifecycle_transition();
    let mut runtime = diagnostics.runtime_status();
    runtime.focused = lifecycle.focused();
    runtime.suspended = lifecycle.suspended();
    runtime.surface = lifecycle.surface();
    diagnostics.set_runtime_status(runtime);
}

struct ProfileRuntime<'a> {
    profile_store: &'a SaveStore<EngineProfile>,
    input: &'a mut Input,
    audio: &'a mut Audio,
    renderer: &'a mut Renderer,
    quality_controller: &'a mut QualityController,
    diagnostics: &'a mut Diagnostics,
}

fn reconcile_profile(
    profile: &mut EngineProfile,
    previous: EngineProfile,
    force_persist: bool,
    runtime: &mut ProfileRuntime<'_>,
) {
    let normalized = profile.normalized();
    let changed = *profile != normalized || normalized != previous;
    *profile = normalized;

    if changed || force_persist {
        profile.apply_input_audio(runtime.input, runtime.audio);
        profile.apply_renderer(runtime.renderer);
        runtime
            .quality_controller
            .set_quality(profile.display.quality);
    }
    if changed || force_persist {
        let save = SaveEnvelope::new(PROFILE_SAVE_VERSION, *profile);
        if let Err(error) = runtime.profile_store.save(&save) {
            runtime.diagnostics.record_save_failure();
            log::warn!("could not persist engine profile: {error}");
        }
    }
}

fn apply_window_profile(window: &Window, profile: EngineProfile, previous: EngineProfile) {
    #[cfg(not(target_arch = "wasm32"))]
    if profile.display.fullscreen != previous.display.fullscreen {
        let fullscreen = profile
            .display
            .fullscreen
            .then(|| winit::window::Fullscreen::Borderless(None));
        window.set_fullscreen(fullscreen);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (window, profile, previous);
}

impl<G: Game> EngineApp<G> {
    fn emit_lifecycle(&mut self, event: LifecycleEvent) {
        record_lifecycle_transition(&mut self.diagnostics, &self.lifecycle);
        if let Some(game) = self.game.as_mut() {
            game.on_lifecycle(event);
        }
    }

    /// Refreshes the engine's gamepad snapshots for this frame from whatever
    /// backend the platform provides. Runs every redraw before simulation.
    fn poll_gamepads(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pads.poll(&mut self.input);
            self.pads.apply_rumbles(&mut self.input);
        }

        #[cfg(target_arch = "wasm32")]
        let pending_rumbles = self.input.drain_rumbles();

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            let Some(window) = web_sys::window() else {
                return;
            };
            // Holes in the returned array (disconnected indices) arrive as
            // `undefined` entries.
            let Ok(pads) = window.navigator().get_gamepads() else {
                self.input.clear_gamepads();
                return;
            };
            let count = pads.length();
            let mut seen = [false; crate::input::MAX_GAMEPADS];
            for (slot, seen_slot) in seen
                .iter_mut()
                .enumerate()
                .take(crate::input::MAX_GAMEPADS.min(count as usize))
            {
                let entry = pads.get(slot as u32);
                if entry.is_null() || entry.is_undefined() {
                    continue;
                }
                let Ok(pad) = entry.dyn_into::<web_sys::Gamepad>() else {
                    continue;
                };
                let button_pressed = |button_index: usize| -> bool {
                    pad.buttons()
                        .get(button_index as u32)
                        .dyn_ref::<web_sys::GamepadButton>()
                        .is_some_and(web_sys::GamepadButton::pressed)
                };
                let axis = |axis_index: usize| -> f32 {
                    pad.axes()
                        .get(axis_index as u32)
                        .as_f64()
                        .map(|value| value as f32)
                        .unwrap_or(0.0)
                };
                let mut buttons = [false; 16];
                for (source, target) in [
                    (0, PadButton::South),
                    (1, PadButton::East),
                    (2, PadButton::West),
                    (3, PadButton::North),
                    (4, PadButton::LeftShoulder),
                    (5, PadButton::RightShoulder),
                    (8, PadButton::Back),
                    (9, PadButton::Start),
                    (12, PadButton::DpadUp),
                    (13, PadButton::DpadDown),
                    (14, PadButton::DpadLeft),
                    (15, PadButton::DpadRight),
                ] {
                    if button_pressed(source) {
                        buttons[target.index().min(15)] = true;
                    }
                }
                let triggers = axis(6).max(axis(7));
                let frame = GamepadFrame {
                    connected: pad.connected(),
                    buttons,
                    left_stick: Vec2::new(axis(0), -axis(1)),
                    right_stick: Vec2::new(axis(2), -axis(3)),
                    triggers: triggers.clamp(0.0, 1.0),
                };
                self.input.push_gamepad_frame(slot, &frame);
                *seen_slot = frame.connected;

                for request in &pending_rumbles {
                    if request.slot == slot {
                        Self::play_web_rumble(&pad, request);
                    }
                }
            }
            for (slot, connected) in seen.iter().enumerate() {
                if !connected {
                    self.input.push_gamepad_frame(
                        slot,
                        &GamepadFrame {
                            connected: false,
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }

    /// Plays one dual-rumble request on a browser pad through its
    /// `vibrationActuator`, silently ignoring pads without one.
    #[cfg(target_arch = "wasm32")]
    fn play_web_rumble(pad: &web_sys::Gamepad, request: &crate::input::RumbleRequest) {
        let actuator = pad.vibration_actuator();
        let params = web_sys::GamepadEffectParameters::new();
        params.set_duration((request.duration * 1000.0) as u32);
        params.set_strong_magnitude(request.low as f64);
        params.set_weak_magnitude(request.high as f64);
        let _ =
            actuator.play_effect_with_params(web_sys::GamepadHapticEffectType::DualRumble, &params);
    }

    fn start_renderer(&mut self, window: Arc<Window>) {
        if self.init_started {
            return;
        }
        self.init_started = true;
        log::info!("Creating Aurora renderer…");

        #[cfg(not(target_arch = "wasm32"))]
        {
            match pollster::block_on(Renderer::new(window)) {
                Ok(renderer) => {
                    let _ = self.proxy.send_event(UserEvent::RendererReady(renderer));
                }
                Err(error) => {
                    log::error!("{error}");
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match Renderer::new(window).await {
                    Ok(renderer) => {
                        let _ = proxy.send_event(UserEvent::RendererReady(renderer));
                    }
                    Err(error) => {
                        log::error!("{error}");
                    }
                }
            });
        }
    }
}

impl<G: Game> ApplicationHandler<UserEvent> for EngineApp<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            if let Some(event) = self.lifecycle.set_suspended(false) {
                self.time.reset_after_suspend();
                self.quality_controller.reset();
                self.emit_lifecycle(event);
            }
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            return;
        }

        #[cfg(target_arch = "wasm32")]
        crate::web_agent::install();

        let title = self
            .game
            .as_ref()
            .map(|g| g.name().to_string())
            .unwrap_or_else(|| "Aurora Engine".into());

        let window_attrs = {
            let attrs = Window::default_attributes()
                .with_title(title)
                .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));

            #[cfg(target_arch = "wasm32")]
            {
                use wasm_bindgen::JsCast;
                use winit::platform::web::WindowAttributesExtWebSys;

                if let Some(canvas) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.get_element_by_id("aurora-canvas"))
                    .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
                {
                    let scale = web_sys::window()
                        .map(|window| window.device_pixel_ratio())
                        .unwrap_or(1.0);
                    let width = (canvas.client_width().max(1) as f64 * scale).round() as u32;
                    let height = (canvas.client_height().max(1) as f64 * scale).round() as u32;
                    canvas.set_width(width);
                    canvas.set_height(height);
                    attrs.with_canvas(Some(canvas))
                } else {
                    attrs
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                attrs
            }
        };

        let window = match event_loop.create_window(window_attrs) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                log::error!("{}", EngineStartError::WindowCreate(error.to_string()));
                return;
            }
        };
        // Keep native pointer coordinates in the same logical-pixel space as
        // the camera/HUD. The renderer applies the same scale when building
        // its viewport, so selection and asset framing stay aligned on
        // Retina displays.
        self.input.set_scale_factor(window.scale_factor() as f32);
        apply_window_profile(&window, self.profile, EngineProfile::default());

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::{closure::Closure, JsCast};
            use winit::platform::web::WindowExtWebSys;
            if let Some(canvas) = window.canvas() {
                let style = canvas.style();
                let _ = style.set_property("width", "100%");
                let _ = style.set_property("height", "100%");
                let _ = style.set_property("display", "block");
                let _ = style.set_property("background", "#0a0d1a");

                // Last Light uses the standard RTS gesture: left-click to
                // select, right-click to command. Browsers otherwise reserve
                // right-click for their context menu, which steals the input
                // before the winit canvas can turn it into a move order.
                let prevent_context_menu =
                    Closure::<dyn FnMut(web_sys::MouseEvent)>::new(|event: web_sys::MouseEvent| {
                        event.prevent_default()
                    });
                let _ = canvas.add_event_listener_with_callback(
                    "contextmenu",
                    prevent_context_menu.as_ref().unchecked_ref(),
                );
                prevent_context_menu.forget();
            }
        }

        self.start_renderer(window.clone());
        self.window = Some(window);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::RendererReady(mut renderer) => {
                #[cfg(target_arch = "wasm32")]
                if let Some(window) = &self.window {
                    let (surface, viewport) = web_surface_size(window);
                    renderer.resize(surface);
                    renderer
                        .camera
                        .set_viewport(viewport.width as f32, viewport.height as f32);
                }
                if let Some(game) = self.game.as_mut() {
                    game.on_start(&mut renderer);
                }
                self.profile.apply_renderer(&mut renderer);
                self.quality_controller
                    .set_quality(self.profile.display.quality);
                let profile_before_hook = self.profile;
                let profile_is_new = self.profile_is_new;
                if let Some(game) = self.game.as_mut() {
                    game.on_profile_loaded(&mut self.profile, profile_is_new);
                }
                let mut profile_runtime = ProfileRuntime {
                    profile_store: &self.profile_store,
                    input: &mut self.input,
                    audio: &mut self.audio,
                    renderer: &mut renderer,
                    quality_controller: &mut self.quality_controller,
                    diagnostics: &mut self.diagnostics,
                };
                reconcile_profile(
                    &mut self.profile,
                    profile_before_hook,
                    profile_is_new,
                    &mut profile_runtime,
                );
                if let Some(window) = &self.window {
                    apply_window_profile(window, self.profile, profile_before_hook);
                }
                self.profile_is_new = false;
                if let Some(event) = self.lifecycle.start() {
                    self.emit_lifecycle(event);
                }
                self.asset_load_queue.mark_all_ready();
                self.renderer = Some(renderer);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                log::info!("Aurora renderer ready — entering main loop");
            }
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(event) = self.lifecycle.set_suspended(true) {
            self.quality_controller.reset();
            self.emit_lifecycle(event);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // Input first so RedrawRequested sees this frame's keys/mouse.
        self.input.handle_event(&event);
        if matches!(
            event,
            WindowEvent::KeyboardInput { .. } | WindowEvent::MouseInput { .. }
        ) {
            self.audio.resume();
        }

        match &event {
            WindowEvent::Focused(focused) => {
                if let Some(event) = self.lifecycle.set_focused(*focused) {
                    self.emit_lifecycle(event);
                }
            }
            WindowEvent::Resized(physical_size) => {
                if let Some(event) = self
                    .lifecycle
                    .resize(physical_size.width, physical_size.height)
                {
                    self.emit_lifecycle(event);
                }
            }
            _ => {}
        }

        if let Some(game) = self.game.as_mut() {
            if game.on_event(&event) {
                return;
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                self.emit_lifecycle(LifecycleEvent::Terminating);
                event_loop.exit();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.input.set_scale_factor(scale_factor as f32);
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.set_scale_factor(scale_factor);
                }
            }
            WindowEvent::Resized(physical_size) => {
                #[cfg(target_arch = "wasm32")]
                let _ = physical_size;
                if let Some(renderer) = self.renderer.as_mut() {
                    #[cfg(not(target_arch = "wasm32"))]
                    renderer.resize(physical_size);
                    #[cfg(target_arch = "wasm32")]
                    if let Some(window) = &self.window {
                        let (surface, viewport) = web_surface_size(window);
                        renderer.resize(surface);
                        renderer
                            .camera
                            .set_viewport(viewport.width as f32, viewport.height as f32);
                    }
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if self.lifecycle.suspended() {
                    self.input.begin_frame();
                    return;
                }

                self.poll_gamepads();

                let (Some(renderer), Some(game), Some(window)) = (
                    self.renderer.as_mut(),
                    self.game.as_mut(),
                    self.window.as_ref(),
                ) else {
                    return;
                };

                self.time.tick();

                // Scripted input must land before any game logic runs this
                // frame (on_fixed_update/on_update), or the edge-triggered
                // key/mouse "pressed" state gets cleared by begin_frame()
                // before any game code ever observes it.
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(devtools) = self.devtools.as_mut() {
                    devtools.tick(
                        self.time.elapsed,
                        self.time.delta,
                        &mut self.input,
                        renderer,
                    );
                }

                #[cfg(not(target_arch = "wasm32"))]
                if let Some(agent_server) = self.agent_server.as_mut() {
                    for request in agent_server.poll() {
                        use crate::agent::AgentCommand;
                        let outcome: Result<serde_json::Value, String> = match &request.command {
                            AgentCommand::Ping => Ok(serde_json::json!({
                                "pong": true,
                                "frame": self.time.frame,
                                "elapsed": self.time.elapsed
                            })),
                            AgentCommand::State => Ok(game
                                .agent_state()
                                .map_or(serde_json::Value::Null, |state| state)),
                            AgentCommand::Diagnostics => Ok(serde_json::json!({
                                "frame": self.time.frame,
                                "fps": self.diagnostics.smoothed_fps(),
                                "frame_ms": self.diagnostics.smoothed_frame_ms(),
                                "render_cpu_ms": self.diagnostics.smoothed_render_cpu_ms(),
                            })),
                            AgentCommand::InjectKey { key, down } => {
                                self.input.simulate_key(*key, *down);
                                Ok(serde_json::json!({"injected": "key"}))
                            }
                            AgentCommand::InjectPadButton { slot, button, down } => {
                                self.input.simulate_gamepad_button(*slot, *button, *down);
                                Ok(serde_json::json!({"injected": "pad_button"}))
                            }
                            AgentCommand::InjectPadStick { slot, stick, x, y } => {
                                self.input.simulate_gamepad_stick(
                                    *slot,
                                    *stick,
                                    glam::Vec2::new(*x, *y),
                                );
                                Ok(serde_json::json!({"injected": "pad_stick"}))
                            }
                            AgentCommand::InjectMouseButton { button, down } => {
                                self.input.simulate_mouse_button(*button, *down);
                                Ok(serde_json::json!({"injected": "mouse_button"}))
                            }
                            AgentCommand::InjectMouseMove { x, y } => {
                                self.input.simulate_mouse_position(Vec2::new(*x, *y));
                                Ok(serde_json::json!({"injected": "mouse_move"}))
                            }
                            AgentCommand::InjectScroll { delta } => {
                                self.input.simulate_scroll(*delta);
                                Ok(serde_json::json!({"injected": "scroll"}))
                            }
                            AgentCommand::Screenshot { path } => {
                                renderer.request_screenshot(std::path::PathBuf::from(path));
                                Ok(serde_json::json!({"screenshot": path}))
                            }
                            AgentCommand::Game { action, args } => game
                                .on_agent_command(action, args)
                                .map(Ok)
                                .unwrap_or_else(|| Err(format!("unsupported action '{action}'"))),
                        };
                        agent_server.respond(request.id, outcome);
                    }
                }

                #[cfg(target_arch = "wasm32")]
                for (action, args) in crate::web_agent::drain(&mut self.input) {
                    if game.on_agent_command(&action, &args).is_none() {
                        log::warn!("web agent game action '{action}' is unsupported");
                    }
                }

                // Fixed steps then variable frame update. A bounded catch-up
                // policy preserves responsive input/rendering after a hitch.
                let mut fixed_steps = 0;
                let max_fixed_steps = self.time.max_fixed_steps_per_frame();
                while fixed_steps < max_fixed_steps {
                    if !self.time.step_fixed() {
                        break;
                    }
                    self.input.begin_fixed_step(fixed_steps);
                    let mut ctx = FrameCtx {
                        time: &mut self.time,
                        input: &self.input,
                        renderer,
                        audio: &mut self.audio,
                        diagnostics: &self.diagnostics,
                        profile: &mut self.profile,
                    };
                    game.on_fixed_update(&mut ctx);
                    fixed_steps += 1;
                }
                if fixed_steps == max_fixed_steps {
                    self.time.discard_fixed_backlog();
                }
                self.input.end_fixed_steps();

                let profile_before_callbacks = self.profile;

                {
                    let mut ctx = FrameCtx {
                        time: &mut self.time,
                        input: &self.input,
                        renderer,
                        audio: &mut self.audio,
                        diagnostics: &self.diagnostics,
                        profile: &mut self.profile,
                    };
                    game.on_update(&mut ctx);
                    game.on_post_update(&mut ctx);
                }

                let mut profile_runtime = ProfileRuntime {
                    profile_store: &self.profile_store,
                    input: &mut self.input,
                    audio: &mut self.audio,
                    renderer,
                    quality_controller: &mut self.quality_controller,
                    diagnostics: &mut self.diagnostics,
                };
                reconcile_profile(
                    &mut self.profile,
                    profile_before_callbacks,
                    false,
                    &mut profile_runtime,
                );
                apply_window_profile(window, self.profile, profile_before_callbacks);

                #[cfg(target_arch = "wasm32")]
                crate::web_agent::publish(game.agent_state());

                match renderer.render(self.time.elapsed) {
                    Ok(()) => {
                        if let Some(event) = self.lifecycle.surface_status(SurfaceStatus::Healthy) {
                            record_lifecycle_transition(&mut self.diagnostics, &self.lifecycle);
                            game.on_lifecycle(event);
                        }
                    }
                    Err(error @ (wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated)) => {
                        let status = match error {
                            wgpu::SurfaceError::Lost => SurfaceStatus::Lost,
                            wgpu::SurfaceError::Outdated => SurfaceStatus::Outdated,
                            _ => unreachable!("surface error pattern is exhaustive"),
                        };
                        if let Some(event) = self.lifecycle.surface_status(status) {
                            record_lifecycle_transition(&mut self.diagnostics, &self.lifecycle);
                            game.on_lifecycle(event);
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        renderer.resize(window.inner_size());
                        #[cfg(target_arch = "wasm32")]
                        {
                            let (surface, viewport) = web_surface_size(window);
                            renderer.resize(surface);
                            renderer
                                .camera
                                .set_viewport(viewport.width as f32, viewport.height as f32);
                        }
                        if let Some(event) = self.lifecycle.surface_status(SurfaceStatus::Healthy) {
                            record_lifecycle_transition(&mut self.diagnostics, &self.lifecycle);
                            game.on_lifecycle(event);
                        }
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        if let Some(event) =
                            self.lifecycle.surface_status(SurfaceStatus::OutOfMemory)
                        {
                            record_lifecycle_transition(&mut self.diagnostics, &self.lifecycle);
                            game.on_lifecycle(event);
                        }
                        log::error!("GPU out of memory");
                        event_loop.exit();
                    }
                    Err(wgpu::SurfaceError::Timeout) => {
                        if let Some(event) = self.lifecycle.surface_status(SurfaceStatus::Timeout) {
                            record_lifecycle_transition(&mut self.diagnostics, &self.lifecycle);
                            game.on_lifecycle(event);
                        }
                        log::warn!("Surface timeout");
                    }
                    Err(wgpu::SurfaceError::Other) => {
                        if let Some(event) = self.lifecycle.surface_status(SurfaceStatus::Other) {
                            record_lifecycle_transition(&mut self.diagnostics, &self.lifecycle);
                            game.on_lifecycle(event);
                        }
                        log::warn!("Surface error (other)");
                    }
                }

                let render_stats = renderer.stats();
                self.diagnostics
                    .record(DiagnosticSnapshot::capture_with_runtime(
                        &self.time,
                        render_stats,
                        &self.asset_load_queue,
                        self.diagnostics.runtime_status(),
                    ));
                if game.adaptive_quality() {
                    if let Some(quality) =
                        self.quality_controller.observe(render_stats.cpu_frame_ms)
                    {
                        renderer.set_quality(quality);
                    }
                }

                // Clear edge-triggered input after the frame has consumed it.
                self.input.begin_frame();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() && !self.lifecycle.suspended() {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }
}

/// Built-in minimal demo (M0 triangle path).
pub struct TriangleDemo {
    pub title: String,
}

impl Default for TriangleDemo {
    fn default() -> Self {
        Self {
            title: "Aurora Engine — Triangle Demo".into(),
        }
    }
}

impl Game for TriangleDemo {
    fn name(&self) -> &str {
        &self.title
    }

    fn on_start(&mut self, renderer: &mut Renderer) {
        renderer.set_debug_triangle(true);
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let hue = (ctx.time.elapsed * 0.05) % 1.0;
        let clear = Color::from_hue(hue).night_blend(0.82);
        ctx.renderer.set_clear_color(clear);
    }
}
