//! Keyboard and mouse input state.

use glam::Vec2;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};

/// A game-defined semantic action. Aurora does not reserve gameplay names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionId(String);
impl ActionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A configurable keyboard binding. More device variants can be added without
/// changing game simulation code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub key: KeyCode,
    pub modifiers: ModifiersState,
}
impl KeyBinding {
    pub fn key(key: KeyCode) -> Self {
        Self {
            key,
            modifiers: ModifiersState::empty(),
        }
    }
    pub fn chord(key: KeyCode, modifiers: ModifiersState) -> Self {
        Self { key, modifiers }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionBinding {
    Key(KeyBinding),
    Pad {
        slot: Option<usize>,
        button: PadButton,
    },
}

#[derive(Debug, Default, Clone)]
pub struct InputMap {
    bindings: HashMap<ActionId, Vec<ActionBinding>>,
}
impl InputMap {
    pub fn bind_key(&mut self, action: ActionId, binding: KeyBinding) {
        self.bindings
            .entry(action)
            .or_default()
            .push(ActionBinding::Key(binding));
    }
    pub fn bind_pad(&mut self, action: ActionId, slot: Option<usize>, button: PadButton) {
        self.bindings
            .entry(action)
            .or_default()
            .push(ActionBinding::Pad { slot, button });
    }

    pub fn bindings(&self, action: &ActionId) -> &[ActionBinding] {
        self.bindings.get(action).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Logical gamepad buttons exposed identically across native and web.
///
/// Names follow the standard mapping (Xbox-style labels), the same model the
/// W3C Gamepad spec and `gilrs` both normalize to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PadButton {
    South,
    East,
    West,
    North,
    LeftShoulder,
    RightShoulder,
    Back,
    Start,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
}

impl PadButton {
    /// Canonical index for this button in a standard-mapping pad snapshot.
    pub const fn index(self) -> usize {
        match self {
            Self::South => 0,
            Self::East => 1,
            Self::West => 2,
            Self::North => 3,
            Self::LeftShoulder => 4,
            Self::RightShoulder => 5,
            Self::Back => 6,
            Self::Start => 7,
            Self::DpadUp => 12,
            Self::DpadDown => 13,
            Self::DpadLeft => 14,
            Self::DpadRight => 15,
        }
    }

    /// All mapped buttons, index order.
    pub const ALL: [PadButton; 12] = [
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::LeftShoulder,
        Self::RightShoulder,
        Self::Back,
        Self::Start,
        Self::DpadUp,
        Self::DpadDown,
        Self::DpadLeft,
        Self::DpadRight,
    ];
}

/// The two analog sticks exposed by a standard gamepad mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadStick {
    Left,
    Right,
}

const PAD_BUTTON_SLOTS: usize = 16;
/// Number of tracked controller slots.
pub const MAX_GAMEPADS: usize = 4;

#[derive(Debug, Default, Clone, Copy)]
struct PadSnapshot {
    connected: bool,
    /// Agent/script input owns this slot until a connected hardware snapshot
    /// supersedes it or focus loss clears all pads.
    synthetic: bool,
    buttons_down: [bool; PAD_BUTTON_SLOTS],
    buttons_pressed: [bool; PAD_BUTTON_SLOTS],
    left_stick: Vec2,
    right_stick: Vec2,
    /// Analog trigger pair, [-1 ..= 1] as released..fully-pressed on +1.
    triggers: f32,
}

impl PadSnapshot {
    fn cleared(&mut self) {
        self.connected = false;
        self.synthetic = false;
        self.buttons_down = [false; PAD_BUTTON_SLOTS];
        self.buttons_pressed = [false; PAD_BUTTON_SLOTS];
        self.left_stick = Vec2::ZERO;
        self.right_stick = Vec2::ZERO;
        self.triggers = 0.0;
    }
}

/// Per-frame input snapshot maintained by the engine.
#[derive(Debug, Clone)]
pub struct Input {
    keys_down: HashSet<KeyCode>,
    keys_pressed: HashSet<KeyCode>,
    keys_released: HashSet<KeyCode>,
    mouse_buttons_down: HashSet<MouseButton>,
    mouse_buttons_pressed: HashSet<MouseButton>,
    mouse_buttons_released: HashSet<MouseButton>,
    pads: [PadSnapshot; MAX_GAMEPADS],
    pad_stick_dead_zone: f32,
    invert_left_y: bool,
    invert_right_y: bool,
    vibration_enabled: bool,
    rumble_queue: RefCell<Vec<RumbleRequest>>,
    /// Cursor position in window pixels (origin top-left).
    pub mouse_position: Vec2,
    /// Mouse delta this frame (pixels).
    pub mouse_delta: Vec2,
    /// Vertical scroll this frame (lines-ish; positive = up).
    pub scroll: f32,
    modifiers: ModifiersState,
    /// Native pointer events arrive in physical pixels. The renderer keeps
    /// its camera/HUD viewport in logical pixels so Retina and browser input
    /// hit the same world-space locations.
    scale_factor: f32,
    prev_mouse: Vec2,
    mouse_initialized: bool,
    /// Fixed-step index for the current rendered frame. Edge-triggered input
    /// is visible to step zero and to the variable update callback, but not to
    /// catch-up steps that would otherwise replay the same key/click edge.
    fixed_step: Option<usize>,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            keys_down: HashSet::new(),
            keys_pressed: HashSet::new(),
            keys_released: HashSet::new(),
            mouse_buttons_down: HashSet::new(),
            mouse_buttons_pressed: HashSet::new(),
            mouse_buttons_released: HashSet::new(),
            pads: [PadSnapshot::default(); MAX_GAMEPADS],
            pad_stick_dead_zone: 0.18,
            invert_left_y: false,
            invert_right_y: false,
            vibration_enabled: true,
            rumble_queue: RefCell::new(Vec::new()),
            mouse_position: Vec2::ZERO,
            mouse_delta: Vec2::ZERO,
            scroll: 0.0,
            modifiers: ModifiersState::empty(),
            scale_factor: 1.0,
            prev_mouse: Vec2::ZERO,
            mouse_initialized: false,
            fixed_step: None,
        }
    }
}

impl Input {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the native window scale used to normalize pointer coordinates.
    /// WASM cursor events already arrive in CSS pixels and ignore this value.
    pub fn set_scale_factor(&mut self, scale: f32) {
        self.scale_factor = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
    }

    /// Call at the start of each frame (clears edge-triggered state).
    pub fn begin_frame(&mut self) {
        self.fixed_step = None;
        self.keys_pressed.clear();
        self.keys_released.clear();
        self.mouse_buttons_pressed.clear();
        self.mouse_buttons_released.clear();
        for pad in &mut self.pads {
            pad.buttons_pressed = [false; PAD_BUTTON_SLOTS];
        }
        self.mouse_delta = Vec2::ZERO;
        self.scroll = 0.0;
    }

    /// Marks the fixed-step phase for one rendered frame.
    pub(crate) fn begin_fixed_step(&mut self, step: usize) {
        self.fixed_step = Some(step);
    }

    /// Returns input reads to normal per-frame semantics after fixed updates.
    pub(crate) fn end_fixed_steps(&mut self) {
        self.fixed_step = None;
    }

    fn edge_visible(&self) -> bool {
        self.fixed_step.is_none_or(|step| step == 0)
    }

    /// Feed a winit window event.
    pub fn handle_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                match event.state {
                    ElementState::Pressed => {
                        if !self.keys_down.contains(&code) {
                            self.keys_pressed.insert(code);
                        }
                        self.keys_down.insert(code);
                    }
                    ElementState::Released => {
                        self.keys_down.remove(&code);
                        self.keys_released.insert(code);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match state {
                ElementState::Pressed => {
                    if !self.mouse_buttons_down.contains(button) {
                        self.mouse_buttons_pressed.insert(*button);
                    }
                    self.mouse_buttons_down.insert(*button);
                }
                ElementState::Released => {
                    self.mouse_buttons_down.remove(button);
                    self.mouse_buttons_released.insert(*button);
                }
            },
            WindowEvent::CursorMoved { position, .. } => {
                // Winit reports physical pixels on every backend. Its web
                // pointer adapter converts CSS offsetX/Y to PhysicalPosition
                // using devicePixelRatio, so WASM needs the same normalization
                // as native input before camera/HUD hit testing.
                let pos =
                    Vec2::new(position.x as f32, position.y as f32) / self.scale_factor.max(1.0);
                if self.mouse_initialized {
                    self.mouse_delta += pos - self.prev_mouse;
                }
                self.prev_mouse = pos;
                self.mouse_position = pos;
                self.mouse_initialized = true;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.scroll += match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => (p.y as f32) * 0.02,
                };
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            // Release events are lost while unfocused; drop held state so keys
            // and buttons don't stay stuck after alt-tabbing away.
            WindowEvent::Focused(false) => {
                self.keys_down.clear();
                self.mouse_buttons_down.clear();
                self.modifiers = ModifiersState::empty();
                self.clear_gamepads();
            }
            _ => {}
        }
    }

    /// Synthesizes a key event as if it came from the window. Used by the
    /// scripted-input debug harness (`AURORA_INPUT_SCRIPT`) to drive a game
    /// without real OS input.
    pub fn simulate_key(&mut self, code: KeyCode, pressed: bool) {
        if pressed {
            if !self.keys_down.contains(&code) {
                self.keys_pressed.insert(code);
            }
            self.keys_down.insert(code);
        } else {
            self.keys_down.remove(&code);
            self.keys_released.insert(code);
        }
    }

    /// Synthesizes a mouse button event. See [`Input::simulate_key`].
    pub fn simulate_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        if pressed {
            if !self.mouse_buttons_down.contains(&button) {
                self.mouse_buttons_pressed.insert(button);
            }
            self.mouse_buttons_down.insert(button);
        } else {
            self.mouse_buttons_down.remove(&button);
            self.mouse_buttons_released.insert(button);
        }
    }

    /// Synthesizes a cursor move to an absolute window position. See
    /// [`Input::simulate_key`].
    pub fn simulate_mouse_position(&mut self, position: Vec2) {
        if self.mouse_initialized {
            self.mouse_delta += position - self.prev_mouse;
        }
        self.prev_mouse = position;
        self.mouse_position = position;
        self.mouse_initialized = true;
    }

    /// Synthesizes a scroll delta. See [`Input::simulate_key`].
    pub fn simulate_scroll(&mut self, delta: f32) {
        self.scroll += delta;
    }

    pub fn key_down(&self, key: KeyCode) -> bool {
        self.keys_down.contains(&key)
    }

    pub fn key_pressed(&self, key: KeyCode) -> bool {
        self.edge_visible() && self.keys_pressed.contains(&key)
    }

    pub fn key_released(&self, key: KeyCode) -> bool {
        self.edge_visible() && self.keys_released.contains(&key)
    }

    pub fn shift_down(&self) -> bool {
        self.modifiers.shift_key()
    }

    /// Treat Command as Control on macOS so RTS control groups remain ergonomic.
    pub fn control_down(&self) -> bool {
        self.modifiers.control_key() || self.modifiers.super_key()
    }

    pub fn action_down(&self, map: &InputMap, action: &ActionId) -> bool {
        map.bindings(action)
            .iter()
            .any(|binding| self.binding_down(*binding))
    }

    pub fn action_pressed(&self, map: &InputMap, action: &ActionId) -> bool {
        map.bindings(action)
            .iter()
            .any(|binding| self.binding_pressed(*binding))
    }

    fn binding_down(&self, binding: ActionBinding) -> bool {
        match binding {
            ActionBinding::Key(binding) => {
                self.key_down(binding.key) && self.modifiers.contains(binding.modifiers)
            }
            ActionBinding::Pad { slot, button } => self
                .resolve_pad_slot(slot)
                .is_some_and(|slot| self.pad_button_down(slot, button)),
        }
    }
    fn binding_pressed(&self, binding: ActionBinding) -> bool {
        match binding {
            ActionBinding::Key(binding) => {
                self.key_pressed(binding.key) && self.modifiers.contains(binding.modifiers)
            }
            ActionBinding::Pad { slot, button } => self
                .resolve_pad_slot(slot)
                .is_some_and(|slot| self.pad_button_pressed(slot, button)),
        }
    }

    pub fn mouse_down(&self, button: MouseButton) -> bool {
        self.mouse_buttons_down.contains(&button)
    }

    pub fn mouse_pressed(&self, button: MouseButton) -> bool {
        self.edge_visible() && self.mouse_buttons_pressed.contains(&button)
    }

    pub fn mouse_released(&self, button: MouseButton) -> bool {
        self.edge_visible() && self.mouse_buttons_released.contains(&button)
    }

    /// Resolves a normalized axis from game-chosen physical keys.
    pub fn axis_from_keys(
        &self,
        up: KeyCode,
        down: KeyCode,
        left: KeyCode,
        right: KeyCode,
    ) -> Vec2 {
        let mut v = Vec2::ZERO;
        if self.key_down(up) {
            v.y += 1.0;
        }
        if self.key_down(down) {
            v.y -= 1.0;
        }
        if self.key_down(left) {
            v.x -= 1.0;
        }
        if self.key_down(right) {
            v.x += 1.0;
        }
        if v.length_squared() > 0.0 {
            v.normalize()
        } else {
            v
        }
    }

    // --- Gamepad surface ---------------------------------------------------

    /// Configures radial dead zone applied to stick reads (default 0.18).
    pub fn set_pad_dead_zone(&mut self, radius: f32) {
        self.pad_stick_dead_zone = finite_clamp(radius, 0.18, 0.0, 0.9);
    }

    pub fn set_pad_axis_inversion(&mut self, left_y: bool, right_y: bool) {
        self.invert_left_y = left_y;
        self.invert_right_y = right_y;
    }

    pub fn set_vibration_enabled(&mut self, enabled: bool) {
        self.vibration_enabled = enabled;
        if !enabled {
            self.rumble_queue.borrow_mut().clear();
        }
    }

    pub fn vibration_enabled(&self) -> bool {
        self.vibration_enabled
    }

    fn pad(&self, slot: usize) -> &PadSnapshot {
        const DISCONNECTED: PadSnapshot = PadSnapshot {
            connected: false,
            synthetic: false,
            buttons_down: [false; PAD_BUTTON_SLOTS],
            buttons_pressed: [false; PAD_BUTTON_SLOTS],
            left_stick: Vec2::ZERO,
            right_stick: Vec2::ZERO,
            triggers: 0.0,
        };
        self.pads.get(slot).unwrap_or(&DISCONNECTED)
    }

    pub fn pad_connected(&self, slot: usize) -> bool {
        self.pad(slot).connected
    }

    /// First connected slot, lowest index first.
    pub fn first_pad(&self) -> Option<usize> {
        (0..MAX_GAMEPADS).find(|&slot| self.pad(slot).connected)
    }

    pub fn connected_pad_count(&self) -> usize {
        self.pads.iter().filter(|pad| pad.connected).count()
    }

    fn resolve_pad_slot(&self, slot: Option<usize>) -> Option<usize> {
        slot.or_else(|| self.first_pad())
            .filter(|&slot| self.pad_connected(slot))
    }

    pub fn pad_button_down(&self, slot: usize, button: PadButton) -> bool {
        let index = button.index();
        index < PAD_BUTTON_SLOTS && self.pad(slot).buttons_down[index]
    }

    pub fn pad_button_pressed(&self, slot: usize, button: PadButton) -> bool {
        let index = button.index();
        self.edge_visible() && index < PAD_BUTTON_SLOTS && self.pad(slot).buttons_pressed[index]
    }

    /// Left stick with the configured dead zone applied; length capped at 1.
    pub fn pad_left_stick(&self, slot: usize) -> Vec2 {
        let mut stick = Self::dead_zone(self.pad(slot).left_stick, self.pad_stick_dead_zone);
        if self.invert_left_y {
            stick.y = -stick.y;
        }
        stick
    }

    /// Right stick with the same dead-zone treatment.
    pub fn pad_right_stick(&self, slot: usize) -> Vec2 {
        let mut stick = Self::dead_zone(self.pad(slot).right_stick, self.pad_stick_dead_zone);
        if self.invert_right_y {
            stick.y = -stick.y;
        }
        stick
    }

    /// Combined analog trigger value (max of both), released = 0.
    pub fn pad_triggers(&self, slot: usize) -> f32 {
        self.pad(slot).triggers.clamp(0.0, 1.0)
    }

    /// Queues a force-feedback impulse for the pad in `slot`. Strengths and
    /// duration clamp to sane ranges; requests for unconnected pads are still
    /// queued — the platform backend simply drops what it cannot play.
    ///
    /// Takes `&self` deliberately: gameplay code receives `Input` behind a
    /// shared reference inside [`FrameCtx`](crate::FrameCtx).
    pub fn rumble(&self, slot: usize, low: f32, high: f32, duration_secs: f32) {
        if !self.vibration_enabled {
            return;
        }
        self.rumble_queue.borrow_mut().push(RumbleRequest {
            slot,
            low: finite_clamp(low, 0.0, 0.0, 1.0),
            high: finite_clamp(high, 0.0, 0.0, 1.0),
            duration: finite_clamp(duration_secs, 0.01, 0.01, 5.0),
        });
    }

    /// Queues a force-feedback impulse for the first connected pad (a no-op
    /// when none is connected).
    pub fn rumble_first(&self, low: f32, high: f32, duration_secs: f32) {
        if let Some(slot) = self.first_pad() {
            self.rumble(slot, low, high, duration_secs);
        }
    }

    /// Takes every pending rumble request; the app shell calls this once per
    /// frame and forwards the requests to the platform haptics backend.
    pub fn drain_rumbles(&self) -> Vec<RumbleRequest> {
        std::mem::take(&mut *self.rumble_queue.borrow_mut())
    }

    /// Merges gamepad state into an axis read with deterministic device
    /// precedence so clashing sources can never fight inside gameplay code:
    /// **keyboard beats d-pad beats stick** (the highest-fidelity source is
    /// the one the player is most deliberately operating).
    pub fn move_axis(
        &self,
        slot: Option<usize>,
        up: KeyCode,
        down: KeyCode,
        left: KeyCode,
        right: KeyCode,
    ) -> Vec2 {
        let keyboard = self.axis_from_keys(up, down, left, right);
        if keyboard != Vec2::ZERO {
            return keyboard;
        }
        let Some(slot) = slot else {
            return Vec2::ZERO;
        };
        let mut dpad = Vec2::ZERO;
        for (button, direction) in [
            (PadButton::DpadUp, Vec2::Y),
            (PadButton::DpadDown, -Vec2::Y),
            (PadButton::DpadLeft, -Vec2::X),
            (PadButton::DpadRight, Vec2::X),
        ] {
            if self.pad_button_down(slot, button) {
                dpad += direction;
            }
        }
        if dpad != Vec2::ZERO {
            return dpad.normalize();
        }
        self.pad_left_stick(slot)
    }

    /// Resolves a controller navigation vector. D-pad input is intentionally
    /// preferred over the stick so a resting stick cannot fight a menu press.
    pub fn navigation_axis(&self, slot: Option<usize>) -> Vec2 {
        let Some(slot) = self.resolve_pad_slot(slot) else {
            return Vec2::ZERO;
        };
        let mut dpad = Vec2::ZERO;
        for (button, direction) in [
            (PadButton::DpadUp, Vec2::Y),
            (PadButton::DpadDown, -Vec2::Y),
            (PadButton::DpadLeft, -Vec2::X),
            (PadButton::DpadRight, Vec2::X),
        ] {
            if self.pad_button_down(slot, button) {
                dpad += direction;
            }
        }
        if dpad != Vec2::ZERO {
            return dpad.normalize_or_zero();
        }
        let stick = self.pad_left_stick(slot);
        if stick.is_finite() {
            stick
        } else {
            Vec2::ZERO
        }
    }

    fn dead_zone(stick: Vec2, radius: f32) -> Vec2 {
        if !stick.is_finite() {
            return Vec2::ZERO;
        }
        let length = stick.length();
        if length <= radius || length <= f32::EPSILON {
            return Vec2::ZERO;
        }
        // Rescale so values just past the dead zone start at zero output,
        // avoiding a pop when leaving rest.
        let usable = (length - radius) / (1.0 - radius);
        stick / length * usable.min(1.0)
    }

    // --- Backend/test entry points -----------------------------------------
}

/// One queued force-feedback impulse. `low`/`high` map to the pad's strong
/// and weak motors (0..=1); duration is seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RumbleRequest {
    pub slot: usize,
    pub low: f32,
    pub high: f32,
    pub duration: f32,
}

/// Test/backend helper struct mirroring one frame of pad state through the
/// public API without exposing engine internals.
#[derive(Debug, Clone, Copy)]
pub struct GamepadFrame {
    pub connected: bool,
    pub buttons: [bool; PAD_BUTTON_SLOTS],
    pub left_stick: Vec2,
    pub right_stick: Vec2,
    pub triggers: f32,
}

impl Default for GamepadFrame {
    fn default() -> Self {
        Self {
            connected: true,
            buttons: [false; PAD_BUTTON_SLOTS],
            left_stick: Vec2::ZERO,
            right_stick: Vec2::ZERO,
            triggers: 0.0,
        }
    }
}

impl Input {
    /// Feeds one backend-agnostic pad frame. Pressed edges are computed the
    /// same way keyboard events are. Tests drive this instead of raw fields.
    pub fn push_gamepad_frame(&mut self, slot: usize, frame: &GamepadFrame) {
        // Hardware backends publish disconnected snapshots for empty slots
        // every frame. Do not let those erase a pad intentionally created by
        // the agent bridge; a connected hardware frame still takes ownership.
        if !frame.connected
            && self
                .pads
                .get(slot)
                .is_some_and(|pad| pad.synthetic && pad.connected)
        {
            return;
        }
        let previous_buttons = self
            .pads
            .get(slot)
            .map_or([false; PAD_BUTTON_SLOTS], |pad| pad.buttons_down);
        let mut pressed_edges = [false; PAD_BUTTON_SLOTS];
        for (index, held_now) in frame.buttons.iter().enumerate() {
            if index < PAD_BUTTON_SLOTS {
                pressed_edges[index] = *held_now && !previous_buttons[index];
            }
        }
        if let Some(target) = self.pads.get_mut(slot) {
            let previous_pressed = target.buttons_pressed;
            *target = PadSnapshot {
                connected: frame.connected,
                synthetic: false,
                buttons_down: frame.buttons,
                buttons_pressed: std::array::from_fn(|index| {
                    previous_pressed[index] || pressed_edges[index]
                }),
                left_stick: frame.left_stick,
                right_stick: frame.right_stick,
                triggers: frame.triggers,
            };
        }
    }

    /// Convenience for tests/gameplay scripting: press or release one button
    /// on the current frame's snapshot.
    pub fn simulate_gamepad_button(&mut self, slot: usize, button: PadButton, pressed: bool) {
        let mut buttons = [false; PAD_BUTTON_SLOTS];
        if let Some(pad) = self.pads.get(slot) {
            buttons.copy_from_slice(&pad.buttons_down);
        }
        buttons[button.index().min(PAD_BUTTON_SLOTS - 1)] = pressed;
        self.apply_buttons(slot, &buttons);
    }

    /// Synthesizes one analog stick sample while preserving the other stick
    /// and all held buttons. Values outside the normalized pad range clamp to
    /// the edge; non-finite samples become neutral instead of poisoning
    /// cursor or camera math.
    pub fn simulate_gamepad_stick(&mut self, slot: usize, stick: PadStick, value: Vec2) {
        let value = if value.is_finite() {
            value.clamp_length_max(1.0)
        } else {
            Vec2::ZERO
        };
        let Some(target) = self.pads.get_mut(slot) else {
            return;
        };
        match stick {
            PadStick::Left => target.left_stick = value,
            PadStick::Right => target.right_stick = value,
        }
        target.connected = true;
        target.synthetic = true;
    }

    /// Writes a raw held-button row for `slot`, preserving other axes.
    fn apply_buttons(&mut self, slot: usize, buttons: &[bool; PAD_BUTTON_SLOTS]) {
        if let Some(target) = self.pads.get_mut(slot) {
            for (index, held_now) in buttons.iter().enumerate() {
                if *held_now && !target.buttons_down[index] {
                    target.buttons_pressed[index] = true;
                }
            }
            target.buttons_down.copy_from_slice(buttons);
            target.connected = true;
            target.synthetic = true;
        }
    }

    /// Disconnects every pad (mirrors focus-loss semantics).
    pub fn clear_gamepads(&mut self) {
        for pad in &mut self.pads {
            pad.cleared();
        }
        self.rumble_queue.borrow_mut().clear();
    }
}

fn finite_clamp(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalPosition;
    use winit::event::DeviceId;

    #[test]
    fn maps_keep_physical_bindings_out_of_game_logic() {
        let mut input = Input::new();
        input.keys_down.insert(KeyCode::ArrowUp);
        let action = ActionId::new("example.move_up");
        let mut map = InputMap::default();
        map.bind_key(action.clone(), KeyBinding::key(KeyCode::ArrowUp));
        assert!(input.action_down(&map, &action));
        assert_eq!(
            input.axis_from_keys(
                KeyCode::ArrowUp,
                KeyCode::ArrowDown,
                KeyCode::ArrowLeft,
                KeyCode::ArrowRight
            ),
            Vec2::Y
        );
    }

    #[test]
    fn focus_loss_releases_held_keys_and_buttons() {
        let mut input = Input::new();
        input.keys_down.insert(KeyCode::KeyW);
        input.mouse_buttons_down.insert(MouseButton::Left);
        input.handle_event(&WindowEvent::Focused(false));
        assert!(!input.key_down(KeyCode::KeyW));
        assert!(!input.mouse_down(MouseButton::Left));
    }

    #[test]
    fn cursor_events_normalize_physical_pixels_to_logical_space() {
        let mut input = Input::new();
        input.set_scale_factor(2.0);
        input.handle_event(&WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(1280.0, 1080.0),
        });
        assert_eq!(input.mouse_position, Vec2::new(640.0, 540.0));
    }

    #[test]
    fn pad_frames_report_edges_like_keyboard_events_do() {
        let mut input = Input::new();
        // Frame one: A held down.
        let mut frame = GamepadFrame::default();
        frame.buttons[PadButton::South.index()] = true;
        input.push_gamepad_frame(0, &frame);
        assert!(input.pad_button_down(0, PadButton::South));
        assert!(input.pad_button_pressed(0, PadButton::South));

        // Frame two (no begin_frame in between would still keep edges honest
        // because edges are computed against the previous snapshot).
        let next = GamepadFrame {
            connected: true,
            ..Default::default()
        };
        input.begin_frame();
        input.push_gamepad_frame(0, &next);
        assert!(!input.pad_button_down(0, PadButton::South));
        assert!(!input.pad_button_pressed(0, PadButton::South));
    }

    #[test]
    fn pad_stick_dead_zone_rescales_from_zero_pop_free() {
        let mut input = Input::new();
        input.set_pad_dead_zone(0.2);

        let mut frame = GamepadFrame {
            left_stick: Vec2::new(0.15, 0.0),
            ..Default::default()
        };
        input.push_gamepad_frame(1, &frame);
        assert_eq!(
            input.pad_left_stick(1),
            Vec2::ZERO,
            "inside the dead zone reads zero"
        );

        frame.left_stick = Vec2::new(0.7, 0.0);
        input.push_gamepad_frame(1, &frame);
        let scaled = input.pad_left_stick(1).x;
        assert!(
            (scaled - 0.625).abs() < 1e-4,
            "rescaled past the zone ({scaled})"
        );

        frame.left_stick = Vec2::new(3.0, 0.0);
        input.push_gamepad_frame(1, &frame);
        assert_eq!(input.pad_left_stick(1).x, 1.0, "clamped to full deflection");
    }

    #[test]
    fn move_axis_blends_pad_stick_dpad_and_keyboard_without_double_fire() {
        let mut input = Input::new();
        let mut frame = GamepadFrame::default();
        frame.buttons[PadButton::DpadRight.index()] = true;
        frame.left_stick = Vec2::new(-1.0, 0.0); // stick held left while dpad right
        input.push_gamepad_frame(2, &frame);

        let axis = input.move_axis(
            Some(2),
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
        );
        assert_eq!(axis, Vec2::X, "dpad beats an opposing held stick");
        assert_eq!(axis.y, 0.0);
    }

    #[test]
    fn disconnected_pads_drop_out_of_first_pad_scan() {
        let mut input = Input::new();
        assert!(input.first_pad().is_none());
        let frame = GamepadFrame::default();
        input.push_gamepad_frame(2, &frame);
        assert_eq!(input.first_pad(), Some(2));
        input.clear_gamepads();
        assert!(!input.pad_connected(2));
        assert!(input.first_pad().is_none());
    }

    #[test]
    fn semantic_actions_read_keyboard_and_gamepad_bindings() {
        let mut input = Input::new();
        let action = ActionId::new("menu.confirm");
        let mut map = InputMap::default();
        map.bind_key(action.clone(), KeyBinding::key(KeyCode::Enter));
        map.bind_pad(action.clone(), None, PadButton::South);

        input.simulate_gamepad_button(0, PadButton::South, true);
        assert!(input.action_pressed(&map, &action));
    }

    #[test]
    fn navigation_axis_prefers_dpad_and_resets_after_disconnect() {
        let mut input = Input::new();
        let mut buttons = [false; 16];
        buttons[PadButton::DpadRight.index()] = true;
        let frame = GamepadFrame {
            connected: true,
            left_stick: Vec2::new(-0.8, 0.0),
            buttons,
            ..Default::default()
        };
        input.push_gamepad_frame(0, &frame);
        assert_eq!(input.navigation_axis(Some(0)), Vec2::X);
        input.clear_gamepads();
        assert_eq!(input.navigation_axis(Some(0)), Vec2::ZERO);
    }

    #[test]
    fn simulate_button_drives_pressed_edges_for_scripting() {
        let mut input = Input::new();
        input.simulate_gamepad_button(0, PadButton::East, true);
        assert!(input.pad_button_pressed(0, PadButton::East));
        assert!(input.pad_button_down(0, PadButton::East));
    }

    #[test]
    fn simulate_stick_preserves_the_other_axis_and_sanitizes_samples() {
        let mut input = Input::new();
        input.set_pad_dead_zone(0.0);
        input.simulate_gamepad_stick(0, PadStick::Left, Vec2::new(-0.6, 0.4));
        input.simulate_gamepad_stick(0, PadStick::Right, Vec2::new(1.4, 0.0));

        assert_eq!(input.pad_left_stick(0), Vec2::new(-0.6, 0.4));
        assert_eq!(input.pad_right_stick(0), Vec2::new(1.0, 0.0));

        input.simulate_gamepad_stick(0, PadStick::Right, Vec2::new(f32::NAN, 0.0));
        assert_eq!(input.pad_right_stick(0), Vec2::ZERO);
        assert!(input.pad_connected(0));
    }

    #[test]
    fn synthetic_pad_survives_empty_hardware_polls_until_focus_reset() {
        let mut input = Input::new();
        input.simulate_gamepad_button(0, PadButton::South, true);
        input.simulate_gamepad_button(0, PadButton::South, false);
        assert!(input.pad_connected(0));

        input.push_gamepad_frame(
            0,
            &GamepadFrame {
                connected: false,
                ..Default::default()
            },
        );
        assert!(input.pad_connected(0));
        assert!(!input.pad_button_down(0, PadButton::South));

        input.clear_gamepads();
        assert!(!input.pad_connected(0));
    }

    #[test]
    fn fixed_step_edges_are_visible_once_and_return_for_frame_update() {
        let mut input = Input::new();
        input.simulate_key(KeyCode::KeyP, true);
        assert!(
            input.key_pressed(KeyCode::KeyP),
            "frame snapshot sees the edge"
        );

        input.begin_fixed_step(0);
        assert!(
            input.key_pressed(KeyCode::KeyP),
            "first fixed step sees the edge"
        );

        input.begin_fixed_step(1);
        assert!(
            !input.key_pressed(KeyCode::KeyP),
            "catch-up fixed steps must not replay the edge"
        );

        input.end_fixed_steps();
        assert!(
            input.key_pressed(KeyCode::KeyP),
            "variable update still sees the frame snapshot"
        );

        input.begin_frame();
        assert!(
            !input.key_pressed(KeyCode::KeyP),
            "the next frame clears the edge"
        );
    }

    #[test]
    fn synthetic_pad_press_survives_same_frame_release() {
        let mut input = Input::new();
        input.simulate_gamepad_button(0, PadButton::East, true);
        input.simulate_gamepad_button(0, PadButton::East, false);

        assert!(!input.pad_button_down(0, PadButton::East));
        assert!(
            input.pad_button_pressed(0, PadButton::East),
            "press edge survives a same-frame release"
        );
    }

    #[test]
    fn rumble_requests_clamp_queue_and_drain_atomically() {
        let input = Input::new();
        input.rumble(0, 1.7, -0.5, 0.0);
        input.rumble_first(0.25, 0.75, 30.0);
        assert!(
            input.first_pad().is_none(),
            "rumble_first ignores absent pads"
        );
        assert_eq!(
            input.rumble_queue.borrow().len(),
            1,
            "only the explicit slot queued"
        );

        let drained = input.drain_rumbles();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].slot, 0);
        assert_eq!(drained[0].low, 1.0, "strength clamps to 1");
        assert_eq!(drained[0].high, 0.0, "strength clamps to 0");
        assert_eq!(drained[0].duration, 0.01, "duration clamps to the floor");
        assert!(input.drain_rumbles().is_empty(), "drain takes everything");
    }

    #[test]
    fn vibration_preference_suppresses_queued_feedback_and_clears_pending_work() {
        let mut input = Input::new();
        input.rumble(0, 0.5, 0.5, 0.1);
        input.set_vibration_enabled(false);
        assert!(!input.vibration_enabled());
        assert!(input.drain_rumbles().is_empty());

        input.rumble(0, 1.0, 1.0, 0.1);
        assert!(input.drain_rumbles().is_empty());
    }
}
