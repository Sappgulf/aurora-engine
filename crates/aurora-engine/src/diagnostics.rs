//! Lightweight runtime metrics for debug HUDs and automated performance checks.

use crate::loader::AssetLoadQueue;
use crate::platform::SurfaceStatus;
use crate::renderer::RenderStats;
use crate::time::Time;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeStatus {
    pub focused: bool,
    pub suspended: bool,
    pub surface: SurfaceStatus,
    pub lifecycle_transitions: u64,
    pub save_recoveries: u64,
    pub save_failures: u64,
}

impl Default for RuntimeStatus {
    fn default() -> Self {
        Self {
            focused: true,
            suspended: false,
            surface: SurfaceStatus::Healthy,
            lifecycle_transitions: 0,
            save_recoveries: 0,
            save_failures: 0,
        }
    }
}

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
    pub dropped_sprites: usize,
    pub dropped_debug_sprites: usize,
    pub composed_lights: usize,
    pub dropped_lights: usize,
    pub staged_vertices: usize,
    pub staged_indices: usize,
    pub sprite_upload_bytes: usize,
    pub quality: crate::renderer::RenderQuality,
    pub total_assets: usize,
    pub ready_assets: usize,
    pub asset_progress: f32,
    pub failed_assets: usize,
    pub pending_assets: usize,
    pub resident_asset_bytes: usize,
    pub rejected_optional_asset_bytes: usize,
    pub focused: bool,
    pub suspended: bool,
    pub surface: SurfaceStatus,
    pub lifecycle_transitions: u64,
    pub save_recoveries: u64,
    pub save_failures: u64,
}

impl DiagnosticSnapshot {
    pub fn capture(time: &Time, render: RenderStats, assets: &AssetLoadQueue) -> Self {
        Self::capture_with_runtime(time, render, assets, RuntimeStatus::default())
    }

    pub fn capture_with_runtime(
        time: &Time,
        render: RenderStats,
        assets: &AssetLoadQueue,
        runtime: RuntimeStatus,
    ) -> Self {
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
            dropped_sprites: render.dropped_sprites,
            dropped_debug_sprites: render.dropped_debug_sprites,
            composed_lights: render.composed_lights,
            dropped_lights: render.dropped_lights,
            staged_vertices: render.staged_vertices,
            staged_indices: render.staged_indices,
            sprite_upload_bytes: render.sprite_upload_bytes,
            quality: render.quality,
            total_assets: assets.total(),
            ready_assets: assets.ready_count(),
            asset_progress: assets.progress(),
            failed_assets: assets.failed_count(),
            pending_assets: assets.pending_count(),
            resident_asset_bytes: assets.resident_bytes(),
            rejected_optional_asset_bytes: assets.rejected_optional_bytes(),
            focused: runtime.focused,
            suspended: runtime.suspended,
            surface: runtime.surface,
            lifecycle_transitions: runtime.lifecycle_transitions,
            save_recoveries: runtime.save_recoveries,
            save_failures: runtime.save_failures,
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
    runtime_status: RuntimeStatus,
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

    pub fn set_runtime_status(&mut self, status: RuntimeStatus) {
        self.runtime_status = status;
    }

    pub fn runtime_status(&self) -> RuntimeStatus {
        self.runtime_status
    }

    pub fn record_lifecycle_transition(&mut self) {
        self.runtime_status.lifecycle_transitions =
            self.runtime_status.lifecycle_transitions.saturating_add(1);
    }

    pub fn record_save_recovery(&mut self) {
        self.runtime_status.save_recoveries = self.runtime_status.save_recoveries.saturating_add(1);
    }

    pub fn record_save_failure(&mut self) {
        self.runtime_status.save_failures = self.runtime_status.save_failures.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{AssetKey, AssetKind, AssetManifest};
    use crate::loader::AssetLoadQueue;

    #[test]
    fn diagnostics_retain_raw_and_smoothed_values() {
        let mut time = Time::new();
        time.delta = 1.0 / 60.0;
        time.frame = 4;
        let mut manifest = AssetManifest::new();
        manifest
            .insert(
                AssetKey::new("texture.hero").unwrap(),
                AssetKind::Texture,
                "hero.png",
            )
            .unwrap();
        manifest
            .insert(
                AssetKey::new("audio.step").unwrap(),
                AssetKind::Audio,
                "step.ogg",
            )
            .unwrap();
        let mut assets = AssetLoadQueue::from_manifest(&manifest);
        assert_eq!(assets.mark_all_ready(), 2);
        let snapshot = DiagnosticSnapshot::capture(
            &time,
            RenderStats {
                draw_calls: 3,
                dropped_sprites: 2,
                dropped_debug_sprites: 4,
                dropped_lights: 1,
                staged_vertices: 12,
                staged_indices: 18,
                sprite_upload_bytes: 240,
                quality: crate::RenderQuality::Cinematic,
                ..Default::default()
            },
            &assets,
        );
        let mut diagnostics = Diagnostics::default();
        diagnostics.record(snapshot);
        assert_eq!(diagnostics.latest(), Some(snapshot));
        assert!((diagnostics.smoothed_fps() - 60.0).abs() < 0.1);
        assert_eq!(snapshot.draw_calls, 3);
        assert_eq!(snapshot.dropped_sprites, 2);
        assert_eq!(snapshot.dropped_debug_sprites, 4);
        assert_eq!(snapshot.dropped_lights, 1);
        assert_eq!(snapshot.staged_vertices, 12);
        assert_eq!(snapshot.staged_indices, 18);
        assert_eq!(snapshot.sprite_upload_bytes, 240);
        assert_eq!(snapshot.quality, crate::RenderQuality::Cinematic);
        assert_eq!(snapshot.pending_assets, 0);
        assert_eq!(snapshot.resident_asset_bytes, 0);
        assert_eq!(snapshot.rejected_optional_asset_bytes, 0);
        assert_eq!(snapshot.total_assets, 2);
        assert_eq!(snapshot.ready_assets, 2);

        let runtime = RuntimeStatus {
            focused: false,
            suspended: true,
            surface: crate::SurfaceStatus::Lost,
            lifecycle_transitions: 4,
            save_recoveries: 1,
            save_failures: 2,
        };
        let snapshot = DiagnosticSnapshot::capture_with_runtime(
            &time,
            RenderStats::default(),
            &assets,
            runtime,
        );
        assert!(snapshot.suspended);
        assert_eq!(snapshot.surface, crate::SurfaceStatus::Lost);
        assert_eq!(snapshot.lifecycle_transitions, 4);
        assert_eq!(snapshot.save_recoveries, 1);
        assert_eq!(snapshot.save_failures, 2);
    }
}
