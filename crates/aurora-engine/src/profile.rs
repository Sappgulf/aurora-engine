use serde::{Deserialize, Serialize};

use crate::audio::{Audio, AudioChannel};
use crate::input::Input;
use crate::renderer::{RenderQuality, Renderer};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineProfile {
    pub audio: AudioProfile,
    pub display: DisplayProfile,
    pub controller: ControllerProfile,
    pub accessibility: AccessibilityProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioProfile {
    pub master: f32,
    pub music: f32,
    pub sfx: f32,
    pub ambience: f32,
    pub ui: f32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayProfile {
    pub render_scale: f32,
    pub quality: RenderQuality,
    pub fullscreen: bool,
    pub post_fx_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControllerProfile {
    pub dead_zone: f32,
    pub cursor_sensitivity: f32,
    pub vibration: bool,
    pub invert_left_y: bool,
    pub invert_right_y: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccessibilityProfile {
    pub reduced_motion: bool,
    pub screen_shake: f32,
    pub text_scale: f32,
    pub high_contrast: bool,
}

impl Default for AudioProfile {
    fn default() -> Self {
        Self {
            master: 1.0,
            music: 0.8,
            sfx: 0.85,
            ambience: 0.7,
            ui: 0.9,
            enabled: true,
        }
    }
}

impl Default for DisplayProfile {
    fn default() -> Self {
        Self {
            render_scale: 1.0,
            quality: RenderQuality::Balanced,
            fullscreen: false,
            post_fx_enabled: true,
        }
    }
}

impl Default for ControllerProfile {
    fn default() -> Self {
        Self {
            dead_zone: 0.18,
            cursor_sensitivity: 1.0,
            vibration: true,
            invert_left_y: false,
            invert_right_y: false,
        }
    }
}

impl Default for AccessibilityProfile {
    fn default() -> Self {
        Self {
            reduced_motion: false,
            screen_shake: 1.0,
            text_scale: 1.0,
            high_contrast: false,
        }
    }
}

impl EngineProfile {
    pub fn normalized(self) -> Self {
        Self {
            audio: AudioProfile {
                master: bounded(self.audio.master, 1.0, 0.0, 1.0),
                music: bounded(self.audio.music, 0.8, 0.0, 1.0),
                sfx: bounded(self.audio.sfx, 0.0, 0.0, 1.0),
                ambience: bounded(self.audio.ambience, 0.7, 0.0, 1.0),
                ui: bounded(self.audio.ui, 0.9, 0.0, 1.0),
                enabled: self.audio.enabled,
            },
            display: DisplayProfile {
                render_scale: bounded(self.display.render_scale, 1.0, 0.5, 1.0),
                quality: self.display.quality,
                fullscreen: self.display.fullscreen,
                post_fx_enabled: self.display.post_fx_enabled,
            },
            controller: ControllerProfile {
                dead_zone: bounded(self.controller.dead_zone, 0.18, 0.0, 0.9),
                cursor_sensitivity: bounded(self.controller.cursor_sensitivity, 1.0, 0.5, 1.75),
                vibration: self.controller.vibration,
                invert_left_y: self.controller.invert_left_y,
                invert_right_y: self.controller.invert_right_y,
            },
            accessibility: AccessibilityProfile {
                reduced_motion: self.accessibility.reduced_motion,
                screen_shake: bounded(self.accessibility.screen_shake, 1.0, 0.0, 1.0),
                text_scale: bounded(self.accessibility.text_scale, 1.0, 0.75, 2.0),
                high_contrast: self.accessibility.high_contrast,
            },
        }
    }

    pub fn apply(&self, input: &mut Input, audio: &mut Audio, renderer: &mut Renderer) {
        let profile = self.normalized();
        profile.apply_input_audio(input, audio);
        profile.apply_renderer(renderer);
    }

    pub fn apply_input_audio(&self, input: &mut Input, audio: &mut Audio) {
        let profile = self.normalized();
        let mixer = audio.mixer_mut();
        mixer.set_volume(AudioChannel::Master, profile.audio.master);
        mixer.set_volume(AudioChannel::Music, profile.audio.music);
        mixer.set_volume(AudioChannel::Sfx, profile.audio.sfx);
        mixer.set_volume(AudioChannel::Ambience, profile.audio.ambience);
        mixer.set_volume(AudioChannel::Ui, profile.audio.ui);
        audio.set_enabled(profile.audio.enabled);

        input.set_pad_dead_zone(profile.controller.dead_zone);
        input.set_pad_axis_inversion(
            profile.controller.invert_left_y,
            profile.controller.invert_right_y,
        );
        input.set_vibration_enabled(profile.controller.vibration);
    }

    pub fn apply_renderer(&self, renderer: &mut Renderer) {
        let profile = self.normalized();
        renderer.set_quality(profile.display.quality);
        renderer.set_render_scale(profile.display.render_scale);
        renderer.post_fx.enabled = profile.display.post_fx_enabled;
    }
}

fn bounded(value: f32, fallback: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderQuality;

    #[test]
    fn profile_normalization_clamps_player_values_and_preserves_intent() {
        let profile = EngineProfile {
            audio: AudioProfile {
                master: 2.0,
                music: -1.0,
                sfx: f32::NAN,
                ambience: 0.5,
                ui: 0.4,
                enabled: true,
            },
            display: DisplayProfile {
                render_scale: 0.01,
                quality: RenderQuality::Cinematic,
                fullscreen: true,
                post_fx_enabled: true,
            },
            controller: ControllerProfile {
                dead_zone: 2.0,
                cursor_sensitivity: 3.0,
                vibration: false,
                invert_left_y: true,
                invert_right_y: false,
            },
            accessibility: AccessibilityProfile {
                reduced_motion: true,
                screen_shake: 4.0,
                text_scale: 0.2,
                high_contrast: true,
            },
        };
        let normalized = profile.normalized();
        assert_eq!(normalized.audio.master, 1.0);
        assert_eq!(normalized.audio.music, 0.0);
        assert_eq!(normalized.audio.sfx, 0.0);
        assert_eq!(normalized.controller.dead_zone, 0.9);
        assert_eq!(normalized.controller.cursor_sensitivity, 1.75);
        assert_eq!(normalized.accessibility.screen_shake, 1.0);
        assert_eq!(normalized.accessibility.text_scale, 0.75);
        assert!(normalized.display.fullscreen);
    }

    #[test]
    fn profile_round_trips_through_serde() {
        let profile = EngineProfile::default();
        let json = serde_json::to_string(&profile).unwrap();
        assert_eq!(
            serde_json::from_str::<EngineProfile>(&json).unwrap(),
            profile
        );
    }
}
