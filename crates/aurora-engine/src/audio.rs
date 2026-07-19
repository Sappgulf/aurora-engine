//! Lightweight sound: procedural beeps (native + web).

/// Independent volume lanes. Their values are multiplied with `Master`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioChannel {
    Master,
    Music,
    Sfx,
    Ambience,
    Ui,
}

/// Portable mixer state shared by native Rodio and Web Audio backends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioMixer {
    master: f32,
    music: f32,
    sfx: f32,
    ambience: f32,
    ui: f32,
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self {
            master: 1.0,
            music: 0.8,
            sfx: 0.85,
            ambience: 0.7,
            ui: 0.9,
        }
    }
}

impl AudioMixer {
    pub fn set_volume(&mut self, channel: AudioChannel, volume: f32) {
        *self.volume_mut(channel) = volume.clamp(0.0, 1.0);
    }

    pub fn volume(&self, channel: AudioChannel) -> f32 {
        match channel {
            AudioChannel::Master => self.master,
            AudioChannel::Music => self.music,
            AudioChannel::Sfx => self.sfx,
            AudioChannel::Ambience => self.ambience,
            AudioChannel::Ui => self.ui,
        }
    }

    pub fn effective_volume(&self, channel: AudioChannel) -> f32 {
        if channel == AudioChannel::Master {
            self.master
        } else {
            self.master * self.volume(channel)
        }
    }

    fn volume_mut(&mut self, channel: AudioChannel) -> &mut f32 {
        match channel {
            AudioChannel::Master => &mut self.master,
            AudioChannel::Music => &mut self.music,
            AudioChannel::Sfx => &mut self.sfx,
            AudioChannel::Ambience => &mut self.ambience,
            AudioChannel::Ui => &mut self.ui,
        }
    }
}

/// Simple game audio helper (no asset files required).
pub struct Audio {
    #[cfg(not(target_arch = "wasm32"))]
    inner: native::NativeAudio,
    #[cfg(target_arch = "wasm32")]
    inner: web::WebAudio,
    enabled: bool,
    mixer: AudioMixer,
}

impl Audio {
    pub fn new() -> Self {
        Self {
            inner: Default::default(),
            enabled: true,
            mixer: AudioMixer::default(),
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn mixer(&self) -> &AudioMixer {
        &self.mixer
    }

    pub fn mixer_mut(&mut self) -> &mut AudioMixer {
        &mut self.mixer
    }

    /// Resume browser audio after a user gesture; a no-op on native platforms.
    pub fn resume(&self) {
        self.inner.resume();
    }

    /// Play a short sine beep.
    pub fn beep(&self, frequency_hz: f32, duration_secs: f32, volume: f32) {
        self.beep_on(AudioChannel::Sfx, frequency_hz, duration_secs, volume);
    }

    /// Play a procedural tone routed through a mixer channel.
    pub fn beep_on(
        &self,
        channel: AudioChannel,
        frequency_hz: f32,
        duration_secs: f32,
        volume: f32,
    ) {
        if !self.enabled {
            return;
        }
        self.inner.beep(
            frequency_hz.max(20.0),
            duration_secs.max(0.01),
            (volume * self.mixer.effective_volume(channel)).clamp(0.0, 1.0),
        );
    }

    pub fn collect(&self) {
        self.beep_on(AudioChannel::Sfx, 880.0, 0.07, 0.25);
    }

    pub fn hurt(&self) {
        self.beep_on(AudioChannel::Sfx, 140.0, 0.15, 0.3);
    }

    pub fn start(&self) {
        self.beep_on(AudioChannel::Ui, 523.0, 0.08, 0.2);
        // second note slightly delayed is hard without threads; single chirp is fine
    }

    pub fn win_note(&self) {
        self.beep(660.0, 0.1, 0.22);
    }
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixer_clamps_lanes_and_applies_master_gain() {
        let mut mixer = AudioMixer::default();
        mixer.set_volume(AudioChannel::Master, 0.5);
        mixer.set_volume(AudioChannel::Sfx, 2.0);
        mixer.set_volume(AudioChannel::Music, -1.0);
        assert_eq!(mixer.volume(AudioChannel::Sfx), 1.0);
        assert_eq!(mixer.volume(AudioChannel::Music), 0.0);
        assert_eq!(mixer.effective_volume(AudioChannel::Sfx), 0.5);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::time::Duration;

    use rodio::source::{SineWave, Source};
    use rodio::{OutputStream, OutputStreamHandle, Sink};

    pub struct NativeAudio {
        _stream: Option<OutputStream>,
        handle: Option<OutputStreamHandle>,
    }

    impl Default for NativeAudio {
        fn default() -> Self {
            match OutputStream::try_default() {
                Ok((stream, handle)) => Self {
                    _stream: Some(stream),
                    handle: Some(handle),
                },
                Err(e) => {
                    log::warn!("Audio unavailable: {e}");
                    Self {
                        _stream: None,
                        handle: None,
                    }
                }
            }
        }
    }

    impl NativeAudio {
        pub fn resume(&self) {}

        pub fn beep(&self, frequency_hz: f32, duration_secs: f32, volume: f32) {
            let Some(handle) = &self.handle else {
                return;
            };
            let Ok(sink) = Sink::try_new(handle) else {
                return;
            };
            let src = SineWave::new(frequency_hz)
                .take_duration(Duration::from_secs_f32(duration_secs))
                .amplify(volume)
                .fade_in(Duration::from_millis(5));
            sink.append(src);
            sink.detach();
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;

    pub struct WebAudio {
        context: RefCell<Option<web_sys::AudioContext>>,
    }

    impl Default for WebAudio {
        fn default() -> Self {
            Self {
                context: RefCell::new(None),
            }
        }
    }

    impl WebAudio {
        fn context(&self) -> Option<web_sys::AudioContext> {
            if self.context.borrow().is_none() {
                let context = web_sys::AudioContext::new().ok()?;
                *self.context.borrow_mut() = Some(context);
            }
            self.context.borrow().clone()
        }

        pub fn resume(&self) {
            if let Some(ctx) = self.context() {
                let _ = ctx.resume();
            }
        }

        pub fn beep(&self, frequency_hz: f32, duration_secs: f32, volume: f32) {
            let Some(ctx) = self.context() else {
                return;
            };
            let Ok(osc) = ctx.create_oscillator() else {
                return;
            };
            let Ok(gain) = ctx.create_gain() else {
                return;
            };
            let _ = osc.frequency().set_value(frequency_hz);
            let _ = gain.gain().set_value(volume);
            let _ = osc.connect_with_audio_node(&gain);
            let _ = gain.connect_with_audio_node(&ctx.destination());
            let _ = osc.start();
            let end = ctx.current_time() + duration_secs as f64;
            let _ = gain.gain().exponential_ramp_to_value_at_time(0.001, end);
            let _ = osc.stop_with_when(end);
        }
    }
}
