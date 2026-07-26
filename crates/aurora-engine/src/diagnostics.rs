//! Lightweight runtime metrics for debug HUDs and automated performance checks.

use crate::loader::AssetLoadQueue;
use crate::renderer::RenderStats;
use crate::time::Time;

/// One immutable reading suitable for a HUD or telemetry adapter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiagnosticSnapshot {
    pub frame: u64,
    pub fps: f32,
    pub frame_ms: f32,
    pub simulation_delta_ms: f32,
    pub fixed_steps_executed: usize,
    pub fixed_step_backlog: f32,
    pub fixed_steps_discarded: usize,
    pub render_cpu_ms: f32,
    pub draw_calls: usize,
    pub drawn_sprites: usize,
    pub invalid_sprites: usize,
    pub composed_lights: usize,
    pub asset_progress: f32,
    pub failed_assets: usize,
}

impl DiagnosticSnapshot {
    pub fn capture(time: &Time, render: RenderStats, assets: &AssetLoadQueue) -> Self {
        let frame_ms = time.delta * 1_000.0;
        Self {
            frame: time.frame,
            fps: if time.delta > f32::EPSILON {
                1.0 / time.delta
            } else {
                0.0
            },
            frame_ms,
            simulation_delta_ms: time.fixed_dt * 1_000.0,
            fixed_steps_executed: time.fixed_steps_executed_last_frame(),
            fixed_step_backlog: time.fixed_step_backlog(),
            fixed_steps_discarded: time.fixed_steps_discarded_last_frame(),
            render_cpu_ms: render.cpu_frame_ms,
            draw_calls: render.draw_calls,
            drawn_sprites: render.drawn_sprites,
            invalid_sprites: render.invalid_sprites,
            composed_lights: render.composed_lights,
            asset_progress: assets.progress(),
            failed_assets: assets.failed_count(),
        }
    }
}

/// Exponential moving average that prevents debug HUDs from flickering between
/// instantaneous frame spikes. The latest raw snapshot is always retained.
#[derive(Debug, Default, Clone)]
pub struct Diagnostics {
    latest: Option<DiagnosticSnapshot>,
    smoothed_fps: f32,
    smoothed_frame_ms: f32,
    smoothed_render_cpu_ms: f32,
}

impl Diagnostics {
    pub fn record(&mut self, snapshot: DiagnosticSnapshot) {
        const NEW_SAMPLE_WEIGHT: f32 = 0.1;
        if self.latest.is_none() {
            self.smoothed_fps = snapshot.fps;
            self.smoothed_frame_ms = snapshot.frame_ms;
            self.smoothed_render_cpu_ms = snapshot.render_cpu_ms;
        } else {
            self.smoothed_fps += (snapshot.fps - self.smoothed_fps) * NEW_SAMPLE_WEIGHT;
            self.smoothed_frame_ms +=
                (snapshot.frame_ms - self.smoothed_frame_ms) * NEW_SAMPLE_WEIGHT;
            self.smoothed_render_cpu_ms +=
                (snapshot.render_cpu_ms - self.smoothed_render_cpu_ms) * NEW_SAMPLE_WEIGHT;
        }
        self.latest = Some(snapshot);
    }

    pub fn latest(&self) -> Option<DiagnosticSnapshot> {
        self.latest
    }

    pub fn smoothed_fps(&self) -> f32 {
        self.smoothed_fps
    }

    pub fn smoothed_frame_ms(&self) -> f32 {
        self.smoothed_frame_ms
    }

    pub fn smoothed_render_cpu_ms(&self) -> f32 {
        self.smoothed_render_cpu_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::AssetLoadQueue;

    #[test]
    fn diagnostics_retain_raw_and_smoothed_values() {
        let mut time = Time::new();
        time.delta = 1.0 / 60.0;
        time.frame = 4;
        let snapshot = DiagnosticSnapshot::capture(
            &time,
            RenderStats {
                draw_calls: 3,
                ..Default::default()
            },
            &AssetLoadQueue::default(),
        );
        let mut diagnostics = Diagnostics::default();
        diagnostics.record(snapshot);
        assert_eq!(diagnostics.latest(), Some(snapshot));
        assert!((diagnostics.smoothed_fps() - 60.0).abs() < 0.1);
        assert_eq!(snapshot.draw_calls, 3);
    }
}
