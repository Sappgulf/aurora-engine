//! Frame timing helpers with optional fixed timestep.

use std::time::Duration;

/// Tracks wall-clock time, delta, and fixed-step accumulator.
#[derive(Debug, Clone)]
pub struct Time {
    start: InstantCompat,
    last: InstantCompat,
    /// Seconds since the engine started.
    pub elapsed: f32,
    /// Seconds since the previous frame (clamped).
    pub delta: f32,
    /// Frames rendered so far.
    pub frame: u64,
    /// Fixed simulation step (default 1/60).
    pub fixed_dt: f32,
    accumulator: f32,
    /// Interpolation alpha after fixed steps: `accumulator / fixed_dt`.
    pub alpha: f32,
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
            fixed_dt: 1.0 / 60.0,
            accumulator: 0.0,
            alpha: 0.0,
        }
    }

    /// Advance time; call once per frame.
    pub fn tick(&mut self) {
        let now = InstantCompat::now();
        self.elapsed = now.duration_since(self.start).as_secs_f32();
        self.delta = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.frame = self.frame.saturating_add(1);
        self.accumulator = (self.accumulator + self.delta).min(0.25);
    }

    /// Consume one fixed step if enough time has accumulated.
    /// Call in a loop: `while time.step_fixed() { sim(); }`
    pub fn step_fixed(&mut self) -> bool {
        if self.accumulator >= self.fixed_dt {
            self.accumulator -= self.fixed_dt;
            self.alpha = self.accumulator / self.fixed_dt;
            true
        } else {
            self.alpha = self.accumulator / self.fixed_dt;
            false
        }
    }
}

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
