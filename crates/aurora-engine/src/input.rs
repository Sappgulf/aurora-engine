//! Keyboard and mouse input state.

use glam::Vec2;
use std::collections::HashSet;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

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
        if self.key_down(KeyCode::KeyW) || self.key_down(KeyCode::ArrowUp) {
            v.y += 1.0;
        }
        if self.key_down(KeyCode::KeyS) || self.key_down(KeyCode::ArrowDown) {
            v.y -= 1.0;
        }
        if self.key_down(KeyCode::KeyA) || self.key_down(KeyCode::ArrowLeft) {
            v.x -= 1.0;
        }
        if self.key_down(KeyCode::KeyD) || self.key_down(KeyCode::ArrowRight) {
            v.x += 1.0;
        }
        if v.length_squared() > 0.0 {
            v.normalize()
        } else {
            v
        }
    }
}
