use crate::renderer::RenderQuality;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityControllerConfig {
    pub target_frame_ms: f32,
    pub over_budget_ratio: f32,
    pub under_budget_ratio: f32,
    pub degradation_samples: u32,
    pub recovery_samples: u32,
}

impl Default for QualityControllerConfig {
    fn default() -> Self {
        Self {
            target_frame_ms: 16.67,
            over_budget_ratio: 1.15,
            under_budget_ratio: 0.80,
            degradation_samples: 3,
            recovery_samples: 120,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QualityController {
    config: QualityControllerConfig,
    quality: RenderQuality,
    enabled: bool,
    over_budget_streak: u32,
    under_budget_streak: u32,
}

impl QualityController {
    pub fn new(target_frame_ms: f32) -> Self {
        Self::with_config(QualityControllerConfig {
            target_frame_ms: valid_target(target_frame_ms),
            ..QualityControllerConfig::default()
        })
    }

    pub fn with_config(mut config: QualityControllerConfig) -> Self {
        config.target_frame_ms = valid_target(config.target_frame_ms);
        config.over_budget_ratio = valid_ratio(config.over_budget_ratio, 1.15);
        config.under_budget_ratio = valid_ratio(config.under_budget_ratio, 0.80);
        config.degradation_samples = config.degradation_samples.max(1);
        config.recovery_samples = config.recovery_samples.max(1);
        Self {
            config,
            quality: RenderQuality::Balanced,
            enabled: true,
            over_budget_streak: 0,
            under_budget_streak: 0,
        }
    }

    pub fn observe(&mut self, frame_ms: f32) -> Option<RenderQuality> {
        if !self.enabled || !frame_ms.is_finite() || frame_ms <= 0.0 {
            return None;
        }

        let over_budget = frame_ms > self.config.target_frame_ms * self.config.over_budget_ratio;
        let under_budget = frame_ms < self.config.target_frame_ms * self.config.under_budget_ratio;
        if over_budget {
            self.under_budget_streak = 0;
            self.over_budget_streak = self.over_budget_streak.saturating_add(1);
            if self.over_budget_streak >= self.config.degradation_samples {
                self.over_budget_streak = 0;
                if let Some(next) = lower_quality(self.quality) {
                    self.quality = next;
                    return Some(next);
                }
            }
        } else if under_budget {
            self.over_budget_streak = 0;
            self.under_budget_streak = self.under_budget_streak.saturating_add(1);
            if self.under_budget_streak >= self.config.recovery_samples {
                self.under_budget_streak = 0;
                if let Some(next) = higher_quality(self.quality) {
                    self.quality = next;
                    return Some(next);
                }
            }
        } else {
            self.reset();
        }
        None
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.reset();
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_quality(&mut self, quality: RenderQuality) {
        self.quality = quality;
        self.reset();
    }

    pub fn quality(&self) -> RenderQuality {
        self.quality
    }

    pub fn reset(&mut self) {
        self.over_budget_streak = 0;
        self.under_budget_streak = 0;
    }
}

fn valid_target(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        16.67
    }
}

fn valid_ratio(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

fn lower_quality(quality: RenderQuality) -> Option<RenderQuality> {
    match quality {
        RenderQuality::Cinematic => Some(RenderQuality::Balanced),
        RenderQuality::Balanced => Some(RenderQuality::Performance),
        RenderQuality::Performance => None,
    }
}

fn higher_quality(quality: RenderQuality) -> Option<RenderQuality> {
    match quality {
        RenderQuality::Performance => Some(RenderQuality::Balanced),
        RenderQuality::Balanced => Some(RenderQuality::Cinematic),
        RenderQuality::Cinematic => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderQuality;

    #[test]
    fn quality_drops_after_three_over_budget_samples() {
        let mut controller = QualityController::new(16.67);
        controller.set_quality(RenderQuality::Cinematic);
        assert_eq!(controller.observe(19.5), None);
        assert_eq!(controller.observe(19.5), None);
        assert_eq!(controller.observe(19.5), Some(RenderQuality::Balanced));
    }

    #[test]
    fn quality_recovers_only_after_a_long_under_budget_window() {
        let mut controller = QualityController::new(16.67);
        controller.set_quality(RenderQuality::Performance);
        for _ in 0..119 {
            assert_eq!(controller.observe(10.0), None);
        }
        assert_eq!(controller.observe(10.0), Some(RenderQuality::Balanced));
    }

    #[test]
    fn invalid_samples_and_disabled_adaptation_are_side_effect_free() {
        let mut controller = QualityController::new(16.67);
        controller.set_enabled(false);
        assert_eq!(controller.observe(f32::NAN), None);
        assert_eq!(controller.quality(), RenderQuality::Balanced);
    }
}
