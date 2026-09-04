//! Juice kit: easing curves, tweening, schedulers, hit-stop, and parallax.
//!
//! Everything here is deterministic: the same tick sequence produces the same
//! outputs, matching the engine's simulation-first contracts. Nothing in this
//! module renders on its own — it produces values games feed into sprites,
//! cameras, or audio parameters.

use glam::Vec2;

use crate::Color;

/// Converts an authored motion intensity into a safe runtime amount.
/// Reduced-motion profiles disable the effect entirely; otherwise the value
/// stays within the normalized range used by accessibility settings.
pub fn motion_intensity(intensity: f32, reduced_motion: bool) -> f32 {
    if reduced_motion || !intensity.is_finite() {
        0.0
    } else {
        intensity.clamp(0.0, 1.0)
    }
}

/// Standard easing curves, evaluated at normalized progress `t` in `[0, 1]`.
///
/// Curves map `0 -> 0` and `1 -> 1`; intermediate values are free to
/// overshoot where noted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Easing {
    /// Constant speed.
    #[default]
    Linear,
    SmoothStep,
    SineIn,
    SineOut,
    SineInOut,
    QuadIn,
    QuadOut,
    QuadInOut,
    CubicIn,
    CubicOut,
    CubicInOut,
    QuartOut,
    /// Fast start with a slight overshoot past the destination.
    BackOut,
    /// Springy multi-wave settle; overshoots both sides.
    ElasticOut,
    /// Bounces on arrival like a dropped ball.
    BounceOut,
}

impl Easing {
    /// Evaluates the curve at normalized `t`. Inputs outside `[0, 1]` clamp.
    pub fn ease(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::SmoothStep => t * t * (3.0 - 2.0 * t),
            Self::SineIn => 1.0 - (t * std::f32::consts::FRAC_PI_2).cos(),
            Self::SineOut => (t * std::f32::consts::FRAC_PI_2).sin(),
            Self::SineInOut => -(std::f32::consts::PI * t).cos() * 0.5 + 0.5,
            Self::QuadIn => t * t,
            Self::QuadOut => 1.0 - (1.0 - t) * (1.0 - t),
            Self::QuadInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
            Self::CubicIn => t.powi(3),
            Self::CubicOut => 1.0 - (1.0 - t).powi(3),
            Self::CubicInOut => {
                if t < 0.5 {
                    4.0 * t.powi(3)
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            Self::QuartOut => 1.0 - (1.0 - t).powi(4),
            Self::BackOut => {
                let c = 1.701_58;
                1.0 + (c + 1.0) * (t - 1.0).powi(3) + c * (t - 1.0).powi(2)
            }
            Self::ElasticOut => {
                if t <= 0.0 {
                    0.0
                } else if t >= 1.0 {
                    1.0
                } else {
                    let c4 = std::f32::consts::TAU / 3.0;
                    (2.0_f32).powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0
                }
            }
            Self::BounceOut => {
                const N: f32 = 7.5625;
                const D: f32 = 2.75;
                if t < 1.0 / D {
                    N * t * t
                } else if t < 2.0 / D {
                    let x = t - 1.5 / D;
                    N * x * x + 0.75
                } else if t < 2.5 / D {
                    let x = t - 2.25 / D;
                    N * x * x + 0.9375
                } else {
                    let x = t - 2.625 / D;
                    N * x * x + 0.984375
                }
            }
        }
    }
}

/// Interpolatable value spaces usable by [`Tween`]s.
pub trait TweenValue: Copy {
    fn lerp(a: Self, b: Self, t: f32) -> Self;
}

impl TweenValue for f32 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
}

impl TweenValue for Vec2 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Vec2::lerp(a, b, t)
    }
}

impl TweenValue for Color {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Color::rgba(
            a.r + (b.r - a.r) * t,
            a.g + (b.g - a.g) * t,
            a.b + (b.b - a.b) * t,
            a.a + (b.a - a.a) * t,
        )
    }
}

/// What happens when a tween reaches its final progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopMode {
    /// Play once, then finish.
    #[default]
    Once,
    /// Restart from the beginning forever.
    Repeat,
    /// Alternate direction each pass forever, forming a triangle wave.
    PingPong,
}

/// A timed interpolation between two values.
///
/// Call [`Tween::restart`] to begin playback, feed delta seconds through
/// [`Tween::tick`], and read [`Tween::value`] at any point. Tweens are cheap
/// `Copy` states, so keeping several per entity is normal. An inactive tween
/// reads its starting pose (`from`).
#[derive(Debug, Clone, Copy)]
pub struct Tween<V: TweenValue> {
    from: V,
    to: V,
    duration: f32,
    delay: f32,
    easing: Easing,
    mode: LoopMode,
    elapsed: f32,
    active: bool,
}

impl<V: TweenValue> Tween<V> {
    pub fn new(from: V, to: V) -> Self {
        Self {
            from,
            to,
            duration: 0.25,
            delay: 0.0,
            easing: Easing::Linear,
            mode: LoopMode::Once,
            elapsed: 0.0,
            active: false,
        }
    }

    /// Sets the animated span in seconds (must be positive).
    pub fn duration(mut self, seconds: f32) -> Self {
        self.duration = seconds.max(f32::EPSILON);
        self
    }

    /// Holds the `from` pose this many seconds before animating.
    pub fn delay(mut self, seconds: f32) -> Self {
        self.delay = seconds.max(0.0);
        self
    }

    pub fn ease(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    pub fn mode(mut self, mode: LoopMode) -> Self {
        self.mode = mode;
        self
    }

    /// Resets playback from the beginning and activates the tween.
    pub fn restart(mut self) -> Self {
        self.elapsed = 0.0;
        self.active = true;
        self
    }

    /// Resets playback in place (kept for stored tweens).
    pub fn start(&mut self) {
        self.elapsed = 0.0;
        self.active = true;
    }

    pub fn stop(&mut self) {
        self.active = false;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Advances playback and returns the current value.
    pub fn tick(&mut self, delta_seconds: f32) -> V {
        if self.active {
            self.elapsed += delta_seconds.max(0.0);
            match self.mode {
                LoopMode::Once => {
                    if self.elapsed - self.delay >= self.duration {
                        self.active = false;
                    }
                }
                LoopMode::Repeat | LoopMode::PingPong => {}
            }
        }
        self.value()
    }

    /// Current interpolated value without advancing time. Inactive tweens
    /// read their `from` pose.
    pub fn value(&self) -> V {
        self.sample(self.progress())
    }

    fn progress(&self) -> f32 {
        if !self.active {
            return 0.0;
        }
        let animated = (self.elapsed - self.delay).max(0.0);
        match self.mode {
            LoopMode::Once => (animated / self.duration).min(1.0),
            LoopMode::Repeat => (animated % self.duration) / self.duration,
            LoopMode::PingPong => {
                let cycle = animated % (2.0 * self.duration);
                if cycle <= self.duration {
                    cycle / self.duration
                } else {
                    1.0 - (cycle - self.duration) / self.duration
                }
            }
        }
    }

    fn sample(&self, raw_t: f32) -> V {
        V::lerp(self.from, self.to, self.easing.ease(raw_t))
    }
}

/// Tagged multi-tween runner so gameplay code can fire-and-forget effects.
///
/// Starting a tween under an existing tag replaces that tag's tween, making
/// "re-trigger" behavior one call. Finished `Once` tweens leave the set
/// automatically, keeping iteration bounded.
#[derive(Debug, Clone)]
pub struct TweenRunner<V: TweenValue> {
    entries: Vec<(u64, Tween<V>)>,
}

impl<V: TweenValue> Default for TweenRunner<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: TweenValue> TweenRunner<V> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Starts (or restarts) the tween bound to `tag`, returning its initial value.
    pub fn start(&mut self, tag: u64, tween: Tween<V>) -> V {
        let mut tween = tween;
        tween.start();
        match self
            .entries
            .iter_mut()
            .find(|(existing, _)| *existing == tag)
        {
            Some((_, slot)) => *slot = tween,
            None => self.entries.push((tag, tween)),
        }
        tween.value()
    }

    pub fn cancel(&mut self, tag: u64) {
        self.entries.retain(|(existing, _)| *existing != tag);
    }

    pub fn cancel_all(&mut self) {
        self.entries.clear();
    }

    pub fn is_active(&self, tag: u64) -> bool {
        self.entries
            .iter()
            .any(|(existing, tween)| *existing == tag && tween.is_active())
    }

    /// Advances every entry; finished `Once` tweens drop out of the set.
    pub fn tick(&mut self, delta_seconds: f32) {
        for (_, tween) in &mut self.entries {
            tween.tick(delta_seconds);
        }
        self.entries.retain(|(_, tween)| tween.is_active());
    }

    /// Latest value for `tag`, if present.
    pub fn value(&self, tag: u64) -> Option<V> {
        self.entries
            .iter()
            .find(|(existing, _)| *existing == tag)
            .map(|(_, tween)| tween.value())
    }

    /// Number of live entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Scheduler event produced by [`Scheduler::tick`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScheduledFire {
    pub id: u64,
    /// Seconds into the tick when this timer came due (monotonic diagnostic).
    pub due_within_frame: f32,
}

/// Deterministic timer set: delays, repeats, cancels.
///
/// Timers are keyed by `u64`, so rescheduling an id replaces its old entry.
/// Fires come back sorted by `(due_within_frame, id)` — identical inputs give
/// identical outputs, satisfying replay tooling.
#[derive(Debug, Clone, Default)]
pub struct Scheduler {
    timers: Vec<TimerEntry>,
}

#[derive(Debug, Clone, Copy)]
struct TimerEntry {
    id: u64,
    /// Seconds until the next fire.
    remaining: f32,
    interval: Option<f32>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fires once after `delay` seconds.
    pub fn after(&mut self, id: u64, delay: f32) {
        self.replace(TimerEntry {
            id,
            remaining: delay.max(0.0),
            interval: None,
        });
    }

    /// Fires every `interval` seconds, first firing after `initial_delay`
    /// (an initial delay of zero waits one full interval).
    pub fn every(&mut self, id: u64, interval: f32, initial_delay: f32) {
        self.replace(TimerEntry {
            id,
            remaining: if initial_delay > 0.0 {
                initial_delay.max(0.0)
            } else {
                interval.max(f32::EPSILON)
            },
            interval: Some(interval.max(f32::EPSILON)),
        });
    }

    /// Removes a timer; returns whether one existed.
    pub fn cancel(&mut self, id: u64) -> bool {
        let before = self.timers.len();
        self.timers.retain(|entry| entry.id != id);
        self.timers.len() != before
    }

    pub fn is_scheduled(&self, id: u64) -> bool {
        self.timers.iter().any(|entry| entry.id == id)
    }

    fn replace(&mut self, entry: TimerEntry) {
        self.timers.retain(|existing| existing.id != entry.id);
        self.timers.push(entry);
    }

    /// Advances all timers, returning everything that came due this tick.
    ///
    /// Fires are ordered by `(due_within_frame, id)` regardless of internal
    /// storage order.
    pub fn tick(&mut self, delta_seconds: f32) -> Vec<ScheduledFire> {
        let budget = delta_seconds.max(0.0);
        if budget == 0.0 || self.timers.is_empty() {
            return Vec::new();
        }
        let mut fired: Vec<(f32, u64)> = Vec::new();
        let mut survivors: Vec<TimerEntry> = Vec::with_capacity(self.timers.len());
        for mut entry in self.timers.drain(..) {
            let mut clock = 0.0_f32;
            'drain: loop {
                if entry.remaining > budget - clock {
                    entry.remaining -= budget - clock;
                    break;
                }
                clock += entry.remaining;
                fired.push((clock, entry.id));
                match entry.interval {
                    Some(interval) => entry.remaining = interval,
                    None => {
                        // One-shot consumed: flag it dead so it cannot survive.
                        entry.remaining = f32::NEG_INFINITY;
                        break 'drain;
                    }
                }
            }
            if entry.remaining > 0.0 || entry.interval.is_some() {
                survivors.push(entry);
            }
        }
        self.timers = survivors;
        fired.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        fired
            .into_iter()
            .map(|(due_within_frame, id)| ScheduledFire {
                id,
                due_within_frame,
            })
            .collect()
    }

    /// Number of live timers.
    pub fn len(&self) -> usize {
        self.timers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.timers.is_empty()
    }
}

/// Brief global time freeze for impact punctuation ("hit-stop").
///
/// Feed [`HitStop::filter`] your game's delta; while frozen it returns zero
/// and consumes freeze budget instead, then releases any surplus back to the
/// simulation. Freeze requests stack additively up to a sane cap so runaway
/// effects cannot stall the game forever.
#[derive(Debug, Clone, Copy, Default)]
pub struct HitStop {
    remaining: f32,
}

const HIT_STOP_CAP: f32 = 0.5;

impl HitStop {
    /// Requests `duration` seconds of freeze; overlapping calls extend it.
    pub fn freeze(&mut self, duration: f32) {
        self.remaining = (self.remaining + duration.max(0.0)).min(HIT_STOP_CAP);
    }

    /// Remaining frozen seconds (diagnostics).
    pub fn remaining(&self) -> f32 {
        self.remaining
    }

    /// Returns the effective simulation delta for this frame.
    pub fn filter(&mut self, delta_seconds: f32) -> f32 {
        let dt = delta_seconds.max(0.0);
        if self.remaining > 0.0 {
            if self.remaining >= dt {
                // Whole frame swallowed by the freeze.
                self.remaining -= dt;
                0.0
            } else {
                let released = dt - self.remaining;
                self.remaining = 0.0;
                released
            }
        } else {
            dt
        }
    }
}

/// Scroll offset for an infinitely wrapping background band.
///
/// `factor` is parallax strength: `0` glues the layer to the camera, `1`
/// moves exactly with the world, values between slide slower. The result is
/// the layer origin modulo `span`, ready for seamless tile wrapping — draw
/// tiles covering `origin - span .. origin + viewport_span` and repeat.
pub fn parallax_offset(camera_center: Vec2, factor: f32, span: Vec2) -> Vec2 {
    let factor = factor.clamp(-10.0, 10.0);
    let shifted = camera_center * factor;
    Vec2::new(
        shifted.x.rem_euclid(span.x.max(f32::EPSILON)),
        shifted.y.rem_euclid(span.y.max(f32::EPSILON)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    #[test]
    fn easings_anchor_endpoints_and_stay_in_unit_range_for_monotone_curves() {
        for curve in [
            Easing::Linear,
            Easing::SmoothStep,
            Easing::SineInOut,
            Easing::QuadInOut,
            Easing::CubicOut,
            Easing::QuartOut,
        ] {
            assert_eq!(curve.ease(0.0), 0.0, "{curve:?} at zero");
            assert_eq!(curve.ease(1.0), 1.0, "{curve:?} at one");
            let mid = curve.ease(0.5);
            assert!((0.0..=1.0).contains(&mid), "{curve:?} mid {mid}");
        }
    }

    #[test]
    fn overshooting_curves_actually_overshoot_but_land_exactly() {
        assert_eq!(Easing::BackOut.ease(1.0), 1.0);
        assert!(Easing::BackOut.ease(0.8) > 1.0, "back-out passes the goal");
        assert_eq!(Easing::ElasticOut.ease(1.0), 1.0);
        let elastic_peak = (0..100)
            .map(|step| Easing::ElasticOut.ease(step as f32 / 100.0))
            .fold(f32::MIN, f32::max);
        assert!(
            elastic_peak > 1.02,
            "elastic-out springs past the goal (peak {elastic_peak})"
        );
        let elastic_floor = (0..100)
            .map(|step| Easing::ElasticOut.ease(step as f32 / 100.0))
            .fold(f32::MAX, f32::min);
        assert!(elastic_floor >= -1e-3, "never dips below start");
    }

    #[test]
    fn bounce_out_reflects_below_the_goal_before_landing() {
        assert_eq!(Easing::BounceOut.ease(1.0), 1.0);
        assert!(Easing::BounceOut.ease(0.5) < 1.0);
        assert!(Easing::BounceOut.ease(0.9995) > 0.98);
    }

    #[test]
    fn tween_runs_start_to_finish_respecting_duration_and_ease() {
        let mut tween = Tween::new(0.0_f32, 100.0)
            .duration(1.0)
            .ease(Easing::QuadOut)
            .restart();
        let first = tween.tick(DT);
        assert!((first - 100.0 * Easing::QuadOut.ease(DT)).abs() < 1e-3);

        for _ in 0..80 {
            tween.tick(DT);
        }
        assert!(!tween.is_active(), "one-shot finishes");
        assert_eq!(tween.value(), 0.0, "inactive reads the from pose");
    }

    #[test]
    fn tween_delay_holds_the_from_value_first_then_carries_time_forward() {
        let mut tween = Tween::new(10.0_f32, 20.0)
            .duration(0.5)
            .delay(0.25)
            .restart();
        assert_eq!(tween.tick(0.1), 10.0, "delay holds the start value");

        // Jump past the delay by a chunk; progress starts counting immediately.
        for _ in 0..30 {
            tween.tick(DT);
        }
        assert!(tween.is_active(), "past the delay, animation proceeds");
    }

    #[test]
    fn color_and_vec2_spaces_interpolate_componentwise() {
        let fade = Tween::new(Color::BLACK, Color::rgb(1.0, 0.0, 0.0))
            .duration(1.0)
            .restart();
        let mut half = fade;
        half.tick(0.5);
        let value = half.value();
        assert!(value.g.abs() < 1e-4 && value.b.abs() < 1e-4);
        assert!((value.r - Easing::Linear.ease(0.5)).abs() < 1e-4);

        let mut slide = Tween::new(Vec2::ZERO, Vec2::new(64.0, -32.0))
            .duration(1.0)
            .restart();
        slide.tick(0.25);
        let pos = slide.value();
        assert!((pos.x - 16.0).abs() < 1e-4 && (pos.y + 8.0).abs() < 1e-4);
    }

    #[test]
    fn ping_pong_forms_a_triangle_wave_never_leaving_endpoints() {
        let mut tween = Tween::new(0.0_f32, 10.0)
            .duration(0.25)
            .mode(LoopMode::PingPong)
            .restart();
        let mut peak = f32::MIN;
        let mut trough = f32::MAX;
        for _ in 0..180 {
            let v = tween.tick(DT);
            peak = peak.max(v);
            trough = trough.min(v);
        }
        assert!(peak <= 10.0 + 1e-3, "never above endpoint ({peak})");
        assert!(trough >= -1e-3, "never below start ({trough})");
        assert!(peak > 9.8 && trough < 0.2, "both extremes visited");
    }

    #[test]
    fn repeat_mode_cycles_without_activation_loss() {
        let mut tween = Tween::new(0.0_f32, 1.0)
            .duration(0.2)
            .mode(LoopMode::Repeat)
            .restart();
        for _ in 0..240 {
            tween.tick(DT);
        }
        assert!(tween.is_active(), "repeat never finishes");
    }

    #[test]
    fn runner_replaces_tags_and_prunes_finished_tweens() {
        let mut runner: TweenRunner<f32> = TweenRunner::new();
        runner.start(7, Tween::new(0.0, 1.0).duration(0.05));
        runner.start(7, Tween::new(5.0, 6.0).duration(0.05));
        assert_eq!(runner.value(7), Some(5.0));

        for _ in 0..12 {
            runner.tick(DT);
        }
        assert!(runner.is_empty(), "completed one-shots leave the runner");
        assert_eq!(runner.value(7), None);
    }

    #[test]
    fn scheduler_after_fires_once_every_repeats_until_cancelled() {
        let mut scheduler = Scheduler::new();
        scheduler.after(1, 0.5);
        scheduler.every(2, 0.25, 0.0);

        let mut ones = 0;
        let mut twos = 0;
        for _ in 0..120 {
            for fire in scheduler.tick(DT) {
                match fire.id {
                    1 => ones += 1,
                    2 => twos += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(ones, 1, "after fires exactly once");
        assert!(
            (7..=10).contains(&twos),
            "0.25s interval fires ~8 times over two seconds ({twos})"
        );
        assert!(scheduler.cancel(2));
        assert!(!scheduler.is_scheduled(2));
        assert!(!scheduler.cancel(2), "double cancel reports absence");
    }

    #[test]
    fn scheduler_fire_order_is_deadline_first_id_breaking_ties() {
        let mut scheduler = Scheduler::new();
        scheduler.after(9, 0.2);
        scheduler.after(2, 0.1);
        scheduler.after(5, 0.2);
        let mut order = Vec::new();
        for _ in 0..20 {
            for fire in scheduler.tick(DT) {
                order.push(fire.id);
            }
        }
        assert_eq!(order, vec![2, 5, 9], "earlier deadline first");
    }

    #[test]
    fn rescheduling_an_id_replaces_its_pending_timer() {
        let mut scheduler = Scheduler::new();
        scheduler.after(4, 3.0);
        scheduler.after(4, 0.05);
        let mut fires = 0;
        for _ in 0..120 {
            for fire in scheduler.tick(DT) {
                assert_eq!(fire.id, 4);
                fires += 1;
            }
        }
        assert_eq!(fires, 1, "only the replacement schedule remains");
    }

    #[test]
    fn hit_stop_freezes_time_then_releases_the_surplus() {
        let mut stop = HitStop::default();
        stop.freeze(0.03); // ~2 frames minus a sliver at 60 Hz
        assert_eq!(stop.filter(DT), 0.0, "fully consumed first frame");
        let resumed = stop.filter(DT);
        assert!(resumed > 0.0 && resumed < DT, "surplus returns to sim");
        assert_eq!(stop.filter(DT), DT, "normal time afterwards");
        assert_eq!(stop.remaining(), 0.0);
    }

    #[test]
    fn hit_stop_requests_stack_but_cannot_exceed_the_cap() {
        let mut stop = HitStop::default();
        for _ in 0..50 {
            stop.freeze(0.06);
        }
        assert!(stop.remaining() <= 0.5, "cap bounds runaway stacking");
    }

    #[test]
    fn parallax_offset_wraps_positive_and_slower_factors_shift_less() {
        let far = parallax_offset(Vec2::new(-1500.0, 40.0), 0.2, Vec2::splat(512.0));
        assert!((0.0..512.0).contains(&far.x));
        // Span chosen so the two wrapped results cannot alias.
        let slow = parallax_offset(Vec2::new(700.0, 0.0), 0.1, Vec2::splat(512.0)).x;
        let fast = parallax_offset(Vec2::new(700.0, 0.0), 0.9, Vec2::splat(512.0)).x;
        assert!((slow - fast).abs() > 1.0);
        let glued = parallax_offset(Vec2::new(1000.0, 500.0), 0.0, Vec2::splat(300.0));
        assert_eq!(glued, Vec2::ZERO, "factor zero tracks the camera");
    }

    #[test]
    fn motion_intensity_honors_reduced_motion_and_safe_bounds() {
        assert_eq!(motion_intensity(0.8, false), 0.8);
        assert_eq!(motion_intensity(2.0, false), 1.0);
        assert_eq!(motion_intensity(-1.0, false), 0.0);
        assert_eq!(motion_intensity(1.0, true), 0.0);
        assert_eq!(motion_intensity(f32::NAN, false), 0.0);
    }
}
