//! Keyboard and mouse input state.

use glam::Vec2;
use std::collections::HashSet;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};

/// Semantic controls used by games; physical key bindings remain centralized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Pause,
    Restart,
    TogglePostFx,
    MenuUp,
    MenuDown,
    MenuConfirm,
    MenuBack,
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
    prev_mouse: Vec2,
    mouse_initialized: bool,
}

impl Input {
    pub fn new() -> Self {
        Self::default()
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
                #[cfg(target_arch = "wasm32")]
                let pos = {
                    let scale = web_sys::window()
                        .map(|window| window.device_pixel_ratio() as f32)
                        .unwrap_or(1.0)
                        .max(1.0);
                    pos / scale
                };
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

    pub fn action_down(&self, action: Action) -> bool {
        self.action_keys(action)
            .iter()
            .any(|key| self.key_down(*key))
    }

    pub fn action_pressed(&self, action: Action) -> bool {
        self.action_keys(action)
            .iter()
            .any(|key| self.key_pressed(*key))
    }

    fn action_keys(&self, action: Action) -> &'static [KeyCode] {
        match action {
            Action::MoveUp => &[KeyCode::KeyW, KeyCode::ArrowUp],
            Action::MoveDown => &[KeyCode::KeyS, KeyCode::ArrowDown],
            Action::MoveLeft => &[KeyCode::KeyA, KeyCode::ArrowLeft],
            Action::MoveRight => &[KeyCode::KeyD, KeyCode::ArrowRight],
            Action::Pause => &[KeyCode::Escape],
            Action::Restart => &[KeyCode::KeyR],
            Action::TogglePostFx => &[KeyCode::KeyP],
            Action::MenuUp => &[KeyCode::ArrowUp, KeyCode::KeyW],
            Action::MenuDown => &[KeyCode::ArrowDown, KeyCode::KeyS],
            Action::MenuConfirm => &[KeyCode::Enter, KeyCode::Space],
            Action::MenuBack => &[KeyCode::Escape],
        }
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

    /// WASD / arrow movement vector (normalized if non-zero).
    pub fn axis_wasd(&self) -> Vec2 {
        let mut v = Vec2::ZERO;
        if self.action_down(Action::MoveUp) {
            v.y += 1.0;
        }
        if self.action_down(Action::MoveDown) {
            v.y -= 1.0;
        }
        if self.action_down(Action::MoveLeft) {
            v.x -= 1.0;
        }
        if self.action_down(Action::MoveRight) {
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
    fn actions_keep_physical_bindings_out_of_game_logic() {
        let mut input = Input::new();
        input.keys_down.insert(KeyCode::ArrowUp);
        assert!(input.action_down(Action::MoveUp));
        assert_eq!(input.axis_wasd(), Vec2::Y);
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
