//! Application trait and cross-platform runner (native + WASM).

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::audio::Audio;
use crate::color::Color;
#[cfg(not(target_arch = "wasm32"))]
use crate::devtools::DebugHarness;
use crate::input::Input;
use crate::renderer::Renderer;
use crate::time::Time;

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
}

/// Implement this for your game / demo.
pub trait Game: 'static {
    fn name(&self) -> &str {
        "Aurora Engine"
    }

    /// Called once after the GPU renderer is ready.
    fn on_start(&mut self, _renderer: &mut Renderer) {}

    /// Fixed-timestep simulation (default 60 Hz). Optional.
    fn on_fixed_update(&mut self, _ctx: &mut FrameCtx<'_>) {}

    /// Called every frame before render (variable delta).
    fn on_update(&mut self, ctx: &mut FrameCtx<'_>);

    /// Optional: handle raw window events after engine input is updated.
    /// Return `true` if the event was consumed.
    fn on_event(&mut self, _event: &WindowEvent) -> bool {
        false
    }
}

enum UserEvent {
    RendererReady(Renderer),
}

/// Launch a game on the current platform (desktop window or browser canvas).
pub fn run<G: Game>(game: G) {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            init_logging_wasm();
        } else {
            init_logging_native();
        }
    }

    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .expect("failed to create event loop");
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let proxy = event_loop.create_proxy();

    let app = EngineApp {
        game: Some(game),
        window: None,
        renderer: None,
        time: Time::new(),
        input: Input::new(),
        audio: Audio::new(),
        proxy,
        init_started: false,
        #[cfg(not(target_arch = "wasm32"))]
        devtools: DebugHarness::from_env(),
    };

    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            use winit::platform::web::EventLoopExtWebSys;
            event_loop.spawn_app(app);
        } else {
            let mut app = app;
            event_loop
                .run_app(&mut app)
                .expect("event loop terminated with error");
        }
    }
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
    audio: Audio,
    proxy: EventLoopProxy<UserEvent>,
    init_started: bool,
    #[cfg(not(target_arch = "wasm32"))]
    devtools: Option<DebugHarness>,
}

impl<G: Game> EngineApp<G> {
    fn start_renderer(&mut self, window: Arc<Window>) {
        if self.init_started {
            return;
        }
        self.init_started = true;
        log::info!("Creating Aurora renderer…");

        #[cfg(not(target_arch = "wasm32"))]
        {
            let renderer = pollster::block_on(Renderer::new(window));
            let _ = self.proxy.send_event(UserEvent::RendererReady(renderer));
        }

        #[cfg(target_arch = "wasm32")]
        {
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let renderer = Renderer::new(window).await;
                let _ = proxy.send_event(UserEvent::RendererReady(renderer));
            });
        }
    }
}

impl<G: Game> ApplicationHandler<UserEvent> for EngineApp<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

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

        let window = Arc::new(
            event_loop
                .create_window(window_attrs)
                .expect("failed to create window"),
        );

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
                self.renderer = Some(renderer);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                log::info!("Aurora renderer ready — entering main loop");
            }
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

        if let Some(game) = self.game.as_mut() {
            if game.on_event(&event) {
                return;
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
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

                // Fixed steps then variable frame update. A bounded catch-up
                // policy preserves responsive input/rendering after a hitch.
                const MAX_FIXED_STEPS_PER_FRAME: usize = 5;
                let mut fixed_steps = 0;
                while fixed_steps < MAX_FIXED_STEPS_PER_FRAME {
                    if !self.time.step_fixed() {
                        break;
                    }
                    let mut ctx = FrameCtx {
                        time: &mut self.time,
                        input: &self.input,
                        renderer,
                        audio: &mut self.audio,
                    };
                    game.on_fixed_update(&mut ctx);
                    fixed_steps += 1;
                }
                if fixed_steps == MAX_FIXED_STEPS_PER_FRAME {
                    self.time.discard_fixed_backlog();
                }

                {
                    let mut ctx = FrameCtx {
                        time: &mut self.time,
                        input: &self.input,
                        renderer,
                        audio: &mut self.audio,
                    };
                    game.on_update(&mut ctx);
                }

                match renderer.render(self.time.elapsed) {
                    Ok(()) => {}
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
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
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        log::error!("GPU out of memory");
                        event_loop.exit();
                    }
                    Err(wgpu::SurfaceError::Timeout) => {
                        log::warn!("Surface timeout");
                    }
                    Err(wgpu::SurfaceError::Other) => {
                        log::warn!("Surface error (other)");
                    }
                }

                // Clear edge-triggered input after the frame has consumed it.
                self.input.begin_frame();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.renderer.is_some() {
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
