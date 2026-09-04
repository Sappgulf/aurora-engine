//! Frame timing helpers with optional fixed timestep.

use std::time::Duration;

/// Tracks wall-clock time, delta, and fixed-step accumulator.
#[derive(Debug, Clone)]
pub struct Time {
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
    /// Simulation speed multiplier for `delta` and fixed-step accumulation.
    /// `1.0` is normal speed, `0.0` is paused.
    simulation_speed: f32,
    /// Maximum wall-clock delta sampled each frame before scaling.
    max_delta: f32,
    /// Maximum fixed-step backlog accumulated each frame before older time is
    /// dropped. This keeps long hitches from producing unbounded catch-up.
    max_accumulator: f32,
    /// Maximum number of fixed steps executed per rendered frame.
    max_fixed_steps_per_frame: usize,
    fixed_steps_executed_last_frame: usize,
    fixed_steps_discarded_last_frame: usize,
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
            last: now,
            elapsed: 0.0,
            delta: 0.0,
            frame: 0,
            fixed_dt: 1.0 / 60.0,
            accumulator: 0.0,
            alpha: 0.0,
            simulation_speed: 1.0,
            max_delta: 0.1,
            max_accumulator: 0.25,
            max_fixed_steps_per_frame: 5,
            fixed_steps_executed_last_frame: 0,
            fixed_steps_discarded_last_frame: 0,
        }
    }

    /// Advance time; call once per frame.
    pub fn tick(&mut self) {
        self.fixed_steps_executed_last_frame = 0;
        self.fixed_steps_discarded_last_frame = 0;
        let now = InstantCompat::now();
        let raw_delta = now
            .duration_since(self.last)
            .as_secs_f32()
            .min(self.max_delta);
        self.last = now;
        let speed = self.simulation_speed.max(0.0);
        self.delta = if speed <= 0.0 { 0.0 } else { raw_delta * speed };
        self.elapsed += self.delta;
        self.frame = self.frame.saturating_add(1);
        self.accumulator = (self.accumulator + self.delta).min(self.max_accumulator);
    }

    /// Discard wall-clock and fixed-step state accumulated while the app was
    /// suspended. The next tick starts a fresh frame interval.
    pub fn reset_after_suspend(&mut self) {
        self.last = InstantCompat::now();
        self.delta = 0.0;
        self.accumulator = 0.0;
        self.alpha = 0.0;
        self.fixed_steps_executed_last_frame = 0;
        self.fixed_steps_discarded_last_frame = 0;
    }

    /// Consume one fixed step if enough time has accumulated.
    /// Call in a loop: `while time.step_fixed() { sim(); }`
    pub fn step_fixed(&mut self) -> bool {
        if self.fixed_dt <= f32::EPSILON {
            self.alpha = 0.0;
            return false;
        }
        if self.accumulator >= self.fixed_dt {
            self.accumulator -= self.fixed_dt;
            self.alpha = self.accumulator / self.fixed_dt;
            self.fixed_steps_executed_last_frame =
                self.fixed_steps_executed_last_frame.saturating_add(1);
            true
        } else {
            self.alpha = self.accumulator / self.fixed_dt;
            false
        }
    }

    /// Discard overdue simulation time while preserving the interpolation
    /// remainder. Call this after a bounded fixed-update loop to keep a slow
    /// frame from causing an ever-growing catch-up backlog.
    pub fn discard_fixed_backlog(&mut self) {
        if self.fixed_dt <= f32::EPSILON {
            self.accumulator = 0.0;
            self.alpha = 0.0;
            self.fixed_steps_discarded_last_frame = 0;
            return;
        }
        let dropped = (self.accumulator / self.fixed_dt).floor();
        self.fixed_steps_discarded_last_frame = if dropped.is_finite() {
            if dropped > usize::MAX as f32 {
                usize::MAX
            } else {
                dropped as usize
            }
        } else {
            0
        };
        self.accumulator %= self.fixed_dt;
        self.alpha = self.accumulator / self.fixed_dt;
    }

    /// Pause simulation time progression while keeping rendering running.
    pub fn pause(&mut self) {
        self.simulation_speed = 0.0;
    }

    /// Resume simulation at normal speed.
    pub fn resume(&mut self) {
        self.set_simulation_speed(1.0);
    }

    /// Set simulation speed multiplier applied to `delta`.
    /// Values below `0.0` are clamped to `0.0`.
    pub fn set_simulation_speed(&mut self, speed: f32) {
        if speed.is_finite() {
            self.simulation_speed = speed.max(0.0);
        } else {
            self.simulation_speed = 1.0;
        }
    }

    /// Current simulation speed multiplier.
    pub fn simulation_speed(&self) -> f32 {
        self.simulation_speed
    }

    /// Set the fixed-step catch-up limit for one rendered frame.
    pub fn set_max_fixed_steps_per_frame(&mut self, steps: usize) {
        self.max_fixed_steps_per_frame = steps.max(1);
    }

    /// Maximum fixed steps executed during a single rendered frame.
    pub fn max_fixed_steps_per_frame(&self) -> usize {
        self.max_fixed_steps_per_frame
    }

    /// Set how much wall-clock delta is sampled each frame.
    pub fn set_max_delta(&mut self, max_delta: f32) {
        if max_delta.is_finite() && max_delta > 0.0 {
            self.max_delta = max_delta;
        }
    }

    /// Clamped wall-clock delta sample window used for fixed-step catch-up.
    pub fn max_delta(&self) -> f32 {
        self.max_delta
    }

    /// Set the maximum fixed-step backlog allowed to accumulate.
    pub fn set_max_accumulator(&mut self, max_accumulator: f32) {
        if max_accumulator.is_finite() && max_accumulator > 0.0 {
            self.max_accumulator = max_accumulator;
        }
    }

    /// Maximum fixed-step catch-up budget in seconds.
    pub fn max_accumulator(&self) -> f32 {
        self.max_accumulator
    }

    /// Number of fixed steps executed in the most recent frame's fixed loop.
    pub fn fixed_steps_executed_last_frame(&self) -> usize {
        self.fixed_steps_executed_last_frame
    }

    /// Number of fixed steps discarded when the frame capped its fixed loop.
    pub fn fixed_steps_discarded_last_frame(&self) -> usize {
        self.fixed_steps_discarded_last_frame
    }

    /// Remaining fixed-step backlog after catch-up execution/discard.
    pub fn fixed_step_backlog(&self) -> f32 {
        self.accumulator
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct InstantCompat {
    #[cfg(not(target_arch = "wasm32"))]
    inner: std::time::Instant,
    #[cfg(target_arch = "wasm32")]
    millis: f64,
}

impl InstantCompat {
    pub(crate) fn now() -> Self {
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

    pub(crate) fn duration_since(self, earlier: Self) -> Duration {
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

#[cfg(test)]
mod tests {
    use super::Time;

    #[test]
    fn discarding_backlog_keeps_only_an_interpolation_remainder() {
        let mut time = Time::new();
        time.fixed_dt = 0.02;
        time.accumulator = 0.117;

        time.discard_fixed_backlog();

        assert!((time.accumulator - 0.017).abs() < 0.000_1);
        assert!((time.alpha - 0.85).abs() < 0.001);
    }

    #[test]
    fn reset_after_suspend_clears_stale_frame_and_fixed_step_state() {
        let mut time = Time::new();
        time.delta = 0.1;
        time.accumulator = 0.12;
        time.alpha = 0.5;
        time.fixed_steps_executed_last_frame = 3;
        time.fixed_steps_discarded_last_frame = 2;

        time.reset_after_suspend();

        assert_eq!(time.delta, 0.0);
        assert_eq!(time.accumulator, 0.0);
        assert_eq!(time.alpha, 0.0);
        assert_eq!(time.fixed_steps_executed_last_frame, 0);
        assert_eq!(time.fixed_steps_discarded_last_frame, 0);
    }
}
