//! Frame timing helpers.

use std::time::Duration;

/// Tracks wall-clock time and per-frame delta for the game loop.
#[derive(Debug, Clone)]
pub struct Time {
    start: InstantCompat,
    last: InstantCompat,
    /// Seconds since the engine started.
    pub elapsed: f32,
    /// Seconds since the previous frame.
    pub delta: f32,
    /// Frames rendered so far.
    pub frame: u64,
}

impl Default for Time {
    fn default() -> Self {
        Self::new()
    }
}

impl Time {
    pub fn new() -> Self {
        let now = InstantCompat::now();
        Self {
            start: now,
            last: now,
            elapsed: 0.0,
            delta: 0.0,
            frame: 0,
        }
    }

    /// Advance time; call once per frame.
    pub fn tick(&mut self) {
        let now = InstantCompat::now();
        self.elapsed = now.duration_since(self.start).as_secs_f32();
        self.delta = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.frame = self.frame.saturating_add(1);
    }
}

/// Instant that works on native and wasm (uses performance.now via web-time pattern).
#[derive(Debug, Clone, Copy)]
struct InstantCompat {
    #[cfg(not(target_arch = "wasm32"))]
    inner: std::time::Instant,
    #[cfg(target_arch = "wasm32")]
    millis: f64,
}

impl InstantCompat {
    fn now() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                inner: std::time::Instant::now(),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self {
                millis: js_sys_now(),
            }
        }
    }

    fn duration_since(self, earlier: Self) -> Duration {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.inner.duration_since(earlier.inner)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let ms = (self.millis - earlier.millis).max(0.0);
            Duration::from_secs_f64(ms / 1000.0)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn js_sys_now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}
