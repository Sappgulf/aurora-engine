//! Keyboard and mouse input state.

use glam::Vec2;
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

#[derive(Debug, Default, Clone)]
pub struct InputMap {
    bindings: HashMap<ActionId, Vec<KeyBinding>>,
}
impl InputMap {
    pub fn bind_key(&mut self, action: ActionId, binding: KeyBinding) {
        self.bindings.entry(action).or_default().push(binding);
    }
    pub fn bindings(&self, action: &ActionId) -> &[KeyBinding] {
        self.bindings.get(action).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Per-frame input snapshot maintained by the engine.
#[derive(Debug, Default, Clone)]
pub struct Input {
    keys_down: HashSet<KeyCode>,
    keys_pressed: HashSet<KeyCode>,
    keys_released: HashSet<KeyCode>,
    mouse_buttons_down: HashSet<MouseButton>,
    mouse_buttons_pressed: HashSet<MouseButton>,
    mouse_buttons_released: HashSet<MouseButton>,
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
        self.keys_pressed.clear();
        self.keys_released.clear();
        self.mouse_buttons_pressed.clear();
        self.mouse_buttons_released.clear();
        self.mouse_delta = Vec2::ZERO;
        self.scroll = 0.0;
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
                let pos = Vec2::new(position.x as f32, position.y as f32);
                #[cfg(not(target_arch = "wasm32"))]
                let pos = pos / self.scale_factor.max(1.0);
                // winit's web cursor positions are already in the canvas's
                // CSS (logical) pixels. The renderer's viewport uses that
                // same unit, so applying devicePixelRatio here would halve
                // Retina cursor coordinates and send playfield clicks into
                // unrelated HUD hit boxes.
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
        self.keys_pressed.contains(&key)
    }

    pub fn key_released(&self, key: KeyCode) -> bool {
        self.keys_released.contains(&key)
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

    fn binding_down(&self, binding: KeyBinding) -> bool {
        self.key_down(binding.key) && self.modifiers.contains(binding.modifiers)
    }
    fn binding_pressed(&self, binding: KeyBinding) -> bool {
        self.key_pressed(binding.key) && self.modifiers.contains(binding.modifiers)
    }

    pub fn mouse_down(&self, button: MouseButton) -> bool {
        self.mouse_buttons_down.contains(&button)
    }

    pub fn mouse_pressed(&self, button: MouseButton) -> bool {
        self.mouse_buttons_pressed.contains(&button)
    }

    pub fn mouse_released(&self, button: MouseButton) -> bool {
        self.mouse_buttons_released.contains(&button)
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
