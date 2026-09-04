//! Lightweight sound: procedural beeps (native + web) and file playback (native).

use std::fmt;
use std::path::Path;

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

/// Everything that can go wrong when playing an audio file. Never fatal:
/// callers can decide to log and keep running.
#[derive(Debug)]
pub enum AudioError {
    /// The file could not be opened or read.
    Io(std::io::Error),
    /// The file was opened but no decoder could understand its contents.
    Decode(String),
    /// The current platform has no file playback backend (e.g. web).
    Unsupported(&'static str),
    /// No audio output device is available (e.g. headless CI).
    NoDevice(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "audio i/o error: {err}"),
            Self::Decode(msg) => write!(f, "audio decode failed: {msg}"),
            Self::Unsupported(why) => write!(f, "audio unsupported here: {why}"),
            Self::NoDevice(msg) => write!(f, "no audio output device: {msg}"),
        }
    }
}

impl std::error::Error for AudioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for AudioError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Final loudness for a file sink: its own base volume scaled by the channel's
/// effective (master-multiplied) volume, clamped to the unity range.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn file_effective_volume(base_volume: f32, mixer: &AudioMixer, channel: AudioChannel) -> f32 {
    (base_volume * mixer.effective_volume(channel)).clamp(0.0, 1.0)
}

/// Drops finished sinks and, when still above `max`, discards the oldest ones.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn prune_and_cap<T>(sinks: &mut Vec<T>, is_finished: impl Fn(&T) -> bool, max: usize) {
    sinks.retain(|sink| !is_finished(sink));
    while sinks.len() > max {
        sinks.remove(0);
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

    /// Mutable access to the mixer. Volume changes reach live file sinks on the
    /// next audio call (any beep or file play re-applies pending volumes).
    pub fn mixer_mut(&mut self) -> &mut AudioMixer {
        &mut self.mixer
    }

    /// Resume browser audio after a user gesture; a no-op on native platforms.
    pub fn resume(&self) {
        self.inner.sync_mixer(&self.mixer);
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
        self.inner.sync_mixer(&self.mixer);
        self.inner.beep(
            frequency_hz.max(20.0),
            duration_secs.max(0.01),
            (volume * self.mixer.effective_volume(channel)).clamp(0.0, 1.0),
        );
    }

    /// Begin looping the audio file at `path` through the
    /// [`AudioChannel::Music`] channel, replacing any earlier music.
    ///
    /// The file is decoded by rodio (wav/flac/ogg-vorbis/mp3 with the default
    /// feature set) and repeats forever until [`Audio::stop_music`] or a later
    /// `play_music` swaps it. Loudness follows `mixer().effective_volume(Music)`.
    /// With audio disabled this is a silent `Ok(())`; a missing device yields
    /// [`AudioError::NoDevice`] instead of panicking.
    pub fn play_music(&mut self, path: impl AsRef<Path>) -> Result<(), AudioError> {
        self.inner.sync_mixer(&self.mixer);
        if !self.enabled {
            return Ok(());
        }
        self.inner.play_music(path.as_ref())
    }

    /// Stop the looping music started by [`Audio::play_music`], if any.
    pub fn stop_music(&mut self) {
        self.inner.stop_music();
    }

    /// Play a decoded audio file once on `channel` (typically
    /// [`AudioChannel::Sfx`] or [`AudioChannel::Ui`]).
    ///
    /// `volume` is the clip's base loudness; the audible result is
    /// `volume * channel volume * master`, clamped to `0.0..=1.0`. Finished
    /// one-shots prune themselves automatically. Errors are graceful: see
    /// [`AudioError`].
    pub fn play_sfx_file(
        &mut self,
        path: impl AsRef<Path>,
        channel: AudioChannel,
        volume: f32,
    ) -> Result<(), AudioError> {
        self.inner.sync_mixer(&self.mixer);
        if !self.enabled {
            return Ok(());
        }
        self.inner.play_sfx_file(path.as_ref(), channel, volume)
    }

    /// Whether music started by [`Audio::play_music`] is still sounding.
    pub fn music_playing(&self) -> bool {
        self.inner.music_playing()
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

    #[test]
    fn file_effective_volume_is_base_times_channel_times_master_clamped() {
        let mut mixer = AudioMixer::default();
        mixer.set_volume(AudioChannel::Master, 0.5);
        mixer.set_volume(AudioChannel::Music, 0.5);
        let got = file_effective_volume(0.8, &mixer, AudioChannel::Music);
        assert!((got - 0.2).abs() < 1e-6, "got {got}");
        // Over-amplified bases clamp to unity instead of distorting.
        assert_eq!(file_effective_volume(4.0, &mixer, AudioChannel::Music), 1.0);
        // The Master lane itself only carries the master gain.
        assert_eq!(
            file_effective_volume(1.0, &mixer, AudioChannel::Master),
            0.5
        );
    }

    #[test]
    fn one_shot_pruning_drops_finished_and_caps_oldest() {
        // Finished sinks are pruned; the cap is never hit here.
        let mut sinks = vec![1usize, 2, 3, 4, 5];
        prune_and_cap(&mut sinks, |id| id % 2 == 1, 8);
        assert_eq!(sinks, vec![2, 4]);
        // Nothing finished: the cap drops the oldest (front) entries.
        let mut sinks = vec![1usize, 2, 3, 4, 5];
        prune_and_cap(&mut sinks, |_| false, 3);
        assert_eq!(sinks, vec![3, 4, 5]);
    }

    #[test]
    fn wav_helper_produces_valid_pcm_header() {
        let bytes = sine_wav_bytes(22_050, 0.05, 440.0);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        let fmt_len = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        assert_eq!(fmt_len, 16);
        let format = u16::from_le_bytes(bytes[20..22].try_into().unwrap());
        assert_eq!(format, 1, "PCM");
        let channels = u16::from_le_bytes(bytes[22..24].try_into().unwrap());
        let sample_rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        assert_eq!(channels, 1);
        assert_eq!(sample_rate, 22_050);
        let bits = u16::from_le_bytes(bytes[34..36].try_into().unwrap());
        assert_eq!(bits, 16);
        assert_eq!(&bytes[36..40], b"data");
        let data_len = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        assert_eq!(data_len % 2, 0, "16-bit frames");
        assert_eq!(bytes.len() as u32, 44 + data_len);
    }

    /// Builds a complete 16-bit mono PCM WAV file in memory.
    fn sine_wav_bytes(sample_rate: u32, duration_secs: f32, frequency_hz: f32) -> Vec<u8> {
        let count = (sample_rate as f32 * duration_secs).round() as usize;
        let mut samples = Vec::with_capacity(count);
        for i in 0..count {
            let t = i as f32 / sample_rate as f32;
            let wave = (t * frequency_hz * std::f32::consts::TAU).sin();
            samples.push((wave * 0.5 * i16::MAX as f32) as i16);
        }
        let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
        let data_len = (samples.len() * 2) as u32;
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
        bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
        bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    mod native_tests {
        use super::*;

        fn write_temp_wav() -> std::path::PathBuf {
            let mut path = std::env::temp_dir();
            path.push(format!("aurora_audio_test_{}.wav", std::process::id()));
            std::fs::write(&path, sine_wav_bytes(44_100, 0.05, 440.0))
                .expect("write temp wav file");
            path
        }

        #[test]
        fn file_playback_round_trip_when_device_available() {
            let path = write_temp_wav();
            let mut audio = Audio::new();
            let has_device = match audio.play_sfx_file(&path, AudioChannel::Ui, 0.5) {
                Ok(()) => true,
                // Headless machines have no output device; skip, never fail.
                Err(AudioError::NoDevice(_)) => false,
                Err(err) => panic!("unexpected file playback error: {err}"),
            };
            if !has_device {
                let _ = std::fs::remove_file(&path);
                return;
            }
            assert!(!audio.music_playing());
            audio
                .play_music(&path)
                .expect("music should play once a device is available");
            assert!(audio.music_playing());
            audio.mixer_mut().set_volume(AudioChannel::Music, 0.25);
            audio.stop_music();
            assert!(!audio.music_playing());
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn missing_file_reports_io_error() {
            let mut audio = Audio::new();
            // The file is opened before any device is touched, so this is
            // deterministic even on machines without audio hardware.
            match audio.play_sfx_file("/nonexistent/aurora/clip.wav", AudioChannel::Sfx, 1.0) {
                Err(AudioError::Io(_)) => {}
                other => panic!("expected Io error, got {other:?}"),
            }
        }

        #[test]
        fn undecodable_file_reports_decode_error() {
            let mut path = std::env::temp_dir();
            path.push(format!("aurora_audio_garbage_{}.bin", std::process::id()));
            std::fs::write(&path, b"aurora: definitely not audio").expect("write garbage file");
            let mut audio = Audio::new();
            match audio.play_sfx_file(&path, AudioChannel::Sfx, 1.0) {
                Err(AudioError::Decode(_)) => {}
                other => panic!("expected Decode error, got {other:?}"),
            }
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::cell::{Cell, RefCell};
    use std::fs::File;
    use std::io::BufReader;
    use std::path::Path;
    use std::time::Duration;

    use rodio::source::{SineWave, Source};
    use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

    use super::{file_effective_volume, prune_and_cap, AudioChannel, AudioError, AudioMixer};

    /// Upper bound on tracked one-shot sinks; overflow drops the oldest.
    const MAX_ONE_SHOT_SINKS: usize = 32;

    /// A tracked file sink: the rodio sink plus the info needed to re-apply
    /// mixer volumes and detect completion.
    struct FileSink {
        sink: Sink,
        channel: AudioChannel,
        base_volume: f32,
    }

    impl FileSink {
        fn apply_volume(&self, mixer: &AudioMixer) {
            self.sink
                .set_volume(file_effective_volume(self.base_volume, mixer, self.channel));
        }

        fn is_finished(&self) -> bool {
            self.sink.empty()
        }
    }

    pub struct NativeAudio {
        _stream: Option<OutputStream>,
        handle: Option<OutputStreamHandle>,
        /// Fallback stream created lazily when the eager one failed (headless
        /// boot with a device appearing later). Kept alive here so handles
        /// never dangle.
        file_playback: RefCell<Option<(OutputStream, OutputStreamHandle)>>,
        music: RefCell<Option<FileSink>>,
        one_shots: RefCell<Vec<FileSink>>,
        /// Snapshot of the mixer state already applied to live sinks.
        applied_mixer: Cell<AudioMixer>,
    }

    impl Default for NativeAudio {
        fn default() -> Self {
            match OutputStream::try_default() {
                Ok((stream, handle)) => Self {
                    _stream: Some(stream),
                    handle: Some(handle),
                    file_playback: RefCell::new(None),
                    music: RefCell::new(None),
                    one_shots: RefCell::new(Vec::new()),
                    applied_mixer: Cell::new(AudioMixer::default()),
                },
                Err(e) => {
                    log::warn!("Audio unavailable: {e}");
                    Self {
                        _stream: None,
                        handle: None,
                        file_playback: RefCell::new(None),
                        music: RefCell::new(None),
                        one_shots: RefCell::new(Vec::new()),
                        applied_mixer: Cell::new(AudioMixer::default()),
                    }
                }
            }
        }
    }

    impl NativeAudio {
        pub fn resume(&self) {}

        /// Re-applies mixer volumes to every live file sink once the mixer
        /// actually changed. Cheap enough to call before any audio activity.
        pub fn sync_mixer(&self, mixer: &AudioMixer) {
            if self.applied_mixer.get() == *mixer {
                return;
            }
            self.applied_mixer.set(*mixer);
            if let Some(file_sink) = self.music.borrow().as_ref() {
                file_sink.apply_volume(mixer);
            }
            for file_sink in self.one_shots.borrow().iter() {
                file_sink.apply_volume(mixer);
            }
        }

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

        pub fn play_music(&self, path: &Path) -> Result<(), AudioError> {
            self.stop_music();
            let file_sink = self.new_file_sink(path, AudioChannel::Music, 1.0, true)?;
            *self.music.borrow_mut() = Some(file_sink);
            Ok(())
        }

        pub fn stop_music(&self) {
            if let Some(file_sink) = self.music.borrow_mut().take() {
                file_sink.sink.stop();
            }
        }

        pub fn play_sfx_file(
            &self,
            path: &Path,
            channel: AudioChannel,
            volume: f32,
        ) -> Result<(), AudioError> {
            {
                let mut sinks = self.one_shots.borrow_mut();
                prune_and_cap(&mut *sinks, FileSink::is_finished, MAX_ONE_SHOT_SINKS);
            }
            let file_sink = self.new_file_sink(path, channel, volume.clamp(0.0, 1.0), false)?;
            let mut sinks = self.one_shots.borrow_mut();
            sinks.push(file_sink);
            prune_and_cap(&mut *sinks, |_| false, MAX_ONE_SHOT_SINKS);
            Ok(())
        }

        pub fn music_playing(&self) -> bool {
            let mut music = self.music.borrow_mut();
            let active = matches!(music.as_ref(), Some(file_sink) if !file_sink.is_finished());
            if !active {
                *music = None;
            }
            active
        }

        /// Decode `path` and wrap it in a sink routed through `channel`. The
        /// device is only touched after the file decodes, so bad files report
        /// [`AudioError::Io`]/[`AudioError::Decode`] even on headless machines.
        /// Looping sources repeat forever via `repeat_infinite`.
        fn new_file_sink(
            &self,
            path: &Path,
            channel: AudioChannel,
            base_volume: f32,
            looping: bool,
        ) -> Result<FileSink, AudioError> {
            let file = File::open(path).map_err(AudioError::Io)?;
            let decoder = Decoder::new(BufReader::new(file))
                .map_err(|e| AudioError::Decode(e.to_string()))?;
            let handle = self.playback_handle()?;
            let sink = Sink::try_new(&handle).map_err(|e| AudioError::NoDevice(e.to_string()))?;
            let file_sink = FileSink {
                sink,
                channel,
                base_volume,
            };
            file_sink.apply_volume(&self.applied_mixer.get());
            if looping {
                file_sink.sink.append(decoder.repeat_infinite());
            } else {
                file_sink.sink.append(decoder);
            }
            Ok(file_sink)
        }

        /// The shared output stream handle for file playback. Reuses the eager
        /// stream when it exists; otherwise creates one lazily and caches it
        /// in `self` so the stream outlives every sink built from it.
        fn playback_handle(&self) -> Result<OutputStreamHandle, AudioError> {
            if let Some(handle) = &self.handle {
                return Ok(handle.clone());
            }
            let mut slot = self.file_playback.borrow_mut();
            match slot.as_ref() {
                Some((_, handle)) => Ok(handle.clone()),
                None => {
                    let (stream, handle) = OutputStream::try_default()
                        .map_err(|e| AudioError::NoDevice(e.to_string()))?;
                    let reused = handle.clone();
                    *slot = Some((stream, handle));
                    log::info!("Audio: opened output stream for file playback");
                    Ok(reused)
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::path::Path;

    use super::{AudioChannel, AudioError, AudioMixer};

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

        pub fn sync_mixer(&self, _mixer: &AudioMixer) {}

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
            osc.frequency().set_value(frequency_hz);
            gain.gain().set_value(volume);
            let _ = osc.connect_with_audio_node(&gain);
            let _ = gain.connect_with_audio_node(&ctx.destination());
            let _ = osc.start();
            let end = ctx.current_time() + duration_secs as f64;
            let _ = gain.gain().exponential_ramp_to_value_at_time(0.001, end);
            let _ = osc.stop_with_when(end);
        }

        pub fn play_music(&self, _path: &Path) -> Result<(), AudioError> {
            Err(AudioError::Unsupported(
                "file playback requires the native audio backend",
            ))
        }

        pub fn stop_music(&self) {}

        pub fn play_sfx_file(
            &self,
            _path: &Path,
            _channel: AudioChannel,
            _volume: f32,
        ) -> Result<(), AudioError> {
            Err(AudioError::Unsupported(
                "file playback requires the native audio backend",
            ))
        }

        pub fn music_playing(&self) -> bool {
            false
        }
    }
}
