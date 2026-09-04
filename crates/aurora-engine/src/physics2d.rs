//! Swept kinematic physics for platformers and action games.
//!
//! The resolver follows the classic two-pass integrator (horizontal sweep,
//! then vertical sweep): each axis moves the body's AABB independently
//! against the supplied [`CollisionContext`], so fast bodies cannot tunnel
//! through thin geometry and wall/floor/ceiling contacts are unambiguous for
//! gameplay code. World space matches every other Aurora system: **Y is up**.
//!
//! Gravity integrates here ([`physics_step`]); player intent lives in
//! [`CharacterParams`] and is layered on top via [`step_character`].

use glam::Vec2;

use crate::{Aabb, TileMap};

/// Contact epsilon that keeps resting bodies from re-colliding each frame.
const SKIN: f32 = 0.01;

/// Maximum surface rise per horizontal step before a ramp reads as a wall.
/// At 60 Hz and typical run speeds a 45-degree ramp rises well under this;
/// steep near-vertical ramps correctly block like walls.
pub const SLOPE_WALL_THRESHOLD: f32 = 12.0;

/// How close a body's underside must be to a ramp surface to snap onto it
/// (and fall off it) per tick. Comfortably above per-tick walk displacement.
pub const SLOPE_SNAP: f32 = 8.0;
/// Vertical cross tolerance for one-way ledges: a descending body must start
/// this close above (or above) the ledge top for it to catch the body.
const ONE_WAY_CROSS: f32 = SKIN * 2.0 + f32::EPSILON;
/// How long a body ignores one-way tops after a deliberate drop-through.
const DROP_THROUGH_GRACE: f32 = 0.2;

/// Gravity multiplier applied while a body is submerged: water feels
/// buoyant, not dead.
pub const WATER_GRAVITY_SCALE: f32 = 0.35;

/// Linear drag per second applied to both velocity axes while submerged.
pub const WATER_DRAG: f32 = 2.5;

/// Terminal downward speed while submerged (units per second), clamped
/// ahead of the controller's own terminal-velocity shaping.
pub const WATER_TERMINAL_FALL: f32 = 240.0;

/// Which side of the body a resolved contact touched.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContactSide {
    /// Support beneath the body.
    Floor,
    /// Blockage against the body's left (-X) face.
    WallLeft,
    /// Blockage against the body's right (+X) face.
    WallRight,
    /// Blockage above the body.
    Ceiling,
}

/// Which collider kind produced a contact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContactSurface {
    /// Static world geometry (rect solids or tilemap cells).
    World,
    /// The indexed game-driven platform.
    Platform(usize),
    /// A one-way ledge top caught on descent.
    OneWay,
    /// A ramp surface (snap landing or steep-ramp wall guard).
    Slope,
}

/// One resolved contact reported by [`physics_step_events`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionEvent {
    pub side: ContactSide,
    pub surface: ContactSurface,
}

/// A mobile body resolved against the world every [`physics_step`].
///
/// Position is the body **center**; `half_size` forms its AABB. Contacts from
/// the most recent step are stored directly on the body so controllers can
/// read `on_ground` / wall flags without extra probes.
#[derive(Debug, Clone, Copy)]
pub struct KinematicBody {
    /// Center of the body AABB in world space.
    pub position: Vec2,
    /// Half-extents of the body AABB.
    pub half_size: Vec2,
    /// Linear velocity in world units per second.
    pub velocity: Vec2,
    /// Multiplier applied to world gravity for this body. `0.0` floats.
    pub gravity_scale: f32,
    /// Seconds remaining in a deliberate drop-through of one-way ledges.
    pub drop_through_grace: f32,
    /// True while standing on walkable floor below.
    pub on_ground: bool,
    /// True while pushing into geometry on the left (-X).
    pub on_wall_left: bool,
    /// True while pushing into geometry on the right (+X).
    pub on_wall_right: bool,
    /// True while pressing into a ceiling above.
    pub on_ceiling: bool,
    /// Index into the context's platform slice this body rode last step.
    pub riding: Option<usize>,
    /// Head-bonk corner correction: when an upward sweep is blocked and the
    /// body clears the blocker's edge within this many units, it slides
    /// sideways around the corner instead of stopping. `0.0` disables.
    pub corner_correction: f32,
    /// Ledge-lip step-up: while grounded, horizontal sweeps over a lip at
    /// most this many units tall are mounted automatically. `0.0` disables.
    pub step_height: f32,
    /// True while the body's center is inside a context water volume;
    /// recomputed from scratch every [`physics_step`].
    pub in_water: bool,
}

impl KinematicBody {
    pub fn new(center: Vec2, size: Vec2) -> Self {
        Self {
            position: center,
            half_size: size.max(Vec2::splat(f32::EPSILON)) * 0.5,
            velocity: Vec2::ZERO,
            gravity_scale: 1.0,
            drop_through_grace: 0.0,
            on_ground: false,
            on_wall_left: false,
            on_wall_right: false,
            on_ceiling: false,
            riding: None,
            corner_correction: 10.0,
            step_height: 12.0,
            in_water: false,
        }
    }

    pub fn aabb(&self) -> Aabb {
        Aabb::new(
            self.position - self.half_size,
            self.position + self.half_size,
        )
    }

    /// Opens a timed ignore window for one-way platform tops (drop-through).
    pub fn request_drop_through(&mut self) {
        self.drop_through_grace = DROP_THROUGH_GRACE;
    }
}

/// A solid volume whose motion the game owns (elevator, ferry, piston).
///
/// Supply the platform's post-move bounds plus the offset traveled this tick;
/// bodies riding the surface are carried automatically.
#[derive(Debug, Clone, Copy)]
pub struct Platform {
    /// Post-move world bounds of the platform for this step.
    pub bounds: Aabb,
    /// Displacement made during this step (`final - initial`).
    pub delta: Vec2,
}

/// A linear walkable surface (ramp) spanning a footprint.
///
/// The surface runs from `(bounds.min.x, surface_left)` to
/// `(bounds.max.x, surface_right)`. Ramps below `MAX_SLOPE_STEP` rise per
/// step are walkable; steeper ramps block like walls. Bodies snap to the
/// surface while approaching or resting on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slope {
    /// Footprint; only the x-range and surface heights matter physically.
    pub bounds: Aabb,
    /// Surface height at `bounds.min.x`.
    pub surface_left: f32,
    /// Surface height at `bounds.max.x`.
    pub surface_right: f32,
}

impl Slope {
    pub fn surface_at(&self, x: f32) -> f32 {
        let span = self.bounds.max.x - self.bounds.min.x;
        if span <= f32::EPSILON {
            return self.surface_left;
        }
        let t = (x - self.bounds.min.x) / span;
        self.surface_left + (self.surface_right - self.surface_left) * t
    }

    /// Ratio of rise over run; positive is uphill toward +X.
    pub fn slope_ratio(&self) -> f32 {
        let span = self.bounds.max.x - self.bounds.min.x;
        if span <= f32::EPSILON {
            0.0
        } else {
            (self.surface_right - self.surface_left) / span
        }
    }
}

/// All collision sources consumed by one [`physics_step`].
///
/// Slices are borrowed so games can drain their own scratch pools rather than
/// allocating inside the engine each frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct CollisionContext<'a> {
    /// Fully solid rectangles.
    pub solids: &'a [Aabb],
    /// Top-only ledges: they block descent, never ascent.
    pub one_ways: &'a [Aabb],
    /// Walkable ramps resolved by surface snapping.
    pub slopes: &'a [Slope],
    /// Game-driven movers; riders are carried by their recorded deltas.
    pub platforms: &'a [Platform],
    /// Optional solid-tile grid queried in addition to the rect lists.
    pub tilemap: Option<&'a TileMap>,
    /// Swim volumes: buoyancy applies while a body's center is inside one.
    /// Water never blocks sweeps; it only shapes gravity, drag, and the
    /// terminal fall speed.
    pub water: &'a [Aabb],
}

impl<'a> CollisionContext<'a> {
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Integrates one body through `delta_seconds` against `context`.
///
/// Returns `true` when any axis was blocked (wall, floor, ceiling, or
/// platform) so callers can trigger land/thud feedback. Identical to
/// [`physics_step_events`] minus the contact stream.
pub fn physics_step(
    body: &mut KinematicBody,
    gravity: f32,
    delta_seconds: f32,
    context: &CollisionContext<'_>,
) -> bool {
    let mut events = Vec::new();
    physics_step_events(body, gravity, delta_seconds, context, &mut events)
}

/// Integrates one body through `delta_seconds` against `context`, appending
/// one [`CollisionEvent`] per resolved contact to `events`.
///
/// Behavior matches [`physics_step`] exactly; events are pure extra output
/// (callers clear the Vec between steps). Swept floor/ceiling/wall blocks
/// report the winning collider (world solid, indexed platform, or one-way
/// ledge), slope snap landings report [`ContactSurface::Slope`], and a
/// junction where an AABB floor block and a slope snap resolve on the same
/// tick pushes only the first contact. The zero-descent ground probe renews
/// contact without pushing an event.
pub fn physics_step_events(
    body: &mut KinematicBody,
    gravity: f32,
    delta_seconds: f32,
    context: &CollisionContext<'_>,
    events: &mut Vec<CollisionEvent>,
) -> bool {
    let dt = delta_seconds.max(0.0);
    let mut collided = false;
    // Only one floor contact resolves per tick: at ramp junctions an AABB
    // block and a slope snap can both fire, and the first one wins.
    let mut floor_seen = false;

    body.on_ground = false;
    body.on_wall_left = false;
    body.on_wall_right = false;
    body.on_ceiling = false;
    let mut riding = None;

    if body.drop_through_grace > 0.0 {
        body.drop_through_grace = (body.drop_through_grace - dt).max(0.0);
    }

    // Carry riders along their platform before integrating so this tick's
    // mover motion cannot open a gap beneath a standing body.
    if let Some(index) = body.riding.take() {
        if let Some(platform) = context.platforms.get(index) {
            body.position += platform.delta;
        }
    }

    // Water state is recomputed every step from the body's center; volumes
    // never block sweeps, they only shape integration below.
    body.in_water = context
        .water
        .iter()
        .any(|volume| volume.contains_point(body.position));

    // Vertical acceleration precedes both sweeps so contacts resolve against
    // the true integrated displacement.
    let gravity_multiplier = if body.in_water {
        WATER_GRAVITY_SCALE
    } else {
        1.0
    };
    body.velocity.y -= gravity * body.gravity_scale.max(0.0) * dt * gravity_multiplier;

    // Submersion shapes the whole velocity: buoyant gravity above, then
    // linear drag and a gentle terminal fall speed. Sweeps are untouched.
    if body.in_water {
        let dampen = (1.0 - WATER_DRAG * dt).max(0.0);
        body.velocity *= dampen;
        body.velocity.y = body.velocity.y.max(-WATER_TERMINAL_FALL);
    }

    // --- Horizontal sweep -------------------------------------------------
    let dx = body.velocity.x * dt;
    if dx != 0.0 {
        let y_span = (body.aabb().min.y, body.aabb().max.y);
        match sweep_axis(
            Axis::X(dx),
            body.aabb().min.x,
            body.aabb().max.x,
            y_span,
            false,
            context,
        ) {
            SweepHit::None => body.position.x += dx,
            SweepHit::Blocked { edge, source } => {
                // Ledge-lip step-up: a grounded body mounts short lips
                // instead of stopping dead, so terrain reads as continuous.
                let mut stepped = false;
                if body.step_height > 0.0 && ground_probe(body, context, SKIN * 2.0).is_some() {
                    let original_y = body.position.y;
                    let mut raise = 2.0;
                    while raise <= body.step_height {
                        body.position.y = original_y + raise;
                        let raised_span = (body.aabb().min.y, body.aabb().max.y);
                        let raised = sweep_axis(
                            Axis::X(dx),
                            body.aabb().min.x,
                            body.aabb().max.x,
                            raised_span,
                            false,
                            context,
                        );
                        if matches!(raised, SweepHit::None)
                            && ground_probe(body, context, raise + SKIN * 2.0).is_some()
                        {
                            stepped = true;
                            break;
                        }
                        body.position.y = original_y;
                        raise += 2.0;
                    }
                }
                if stepped {
                    body.position.x += dx;
                } else {
                    if dx > 0.0 {
                        body.position.x = edge - body.half_size.x - SKIN;
                        body.on_wall_right = true;
                    } else {
                        body.position.x = edge + body.half_size.x + SKIN;
                        body.on_wall_left = true;
                    }
                    body.velocity.x = 0.0;
                    events.push(CollisionEvent {
                        side: wall_side(dx),
                        surface: contact_surface(source),
                    });
                    collided = true;
                }
            }
        }
    }

    // Slope steepness guard: a ramp whose own step rise over the upcoming
    // horizontal advancement exceeds SLOPE_WALL_THRESHOLD blocks like a wall.
    // The comparison is surface-to-surface (not surface-to-feet), so a body
    // standing below the line is never confused for a steep climb.
    if !context.slopes.is_empty() && dx != 0.0 && body.velocity.y <= 0.0 {
        for slope in context.slopes {
            let span = (slope.bounds.min.x, slope.bounds.max.x);
            if !spans_overlap(span.0, span.1, (body.aabb().min.x, body.aabb().max.x)) {
                continue;
            }
            let center_clamped = body.position.x.clamp(span.0, span.1);
            let ahead = (body.position.x + dx).clamp(span.0, span.1);
            let rise = (slope.surface_at(ahead) - slope.surface_at(center_clamped)) * dx.signum();
            if rise > SLOPE_WALL_THRESHOLD {
                events.push(CollisionEvent {
                    side: wall_side(dx),
                    surface: ContactSurface::Slope,
                });
                if dx > 0.0 {
                    body.on_wall_right = true;
                } else {
                    body.on_wall_left = true;
                }
                body.velocity.x = 0.0;
                collided = true;
                break;
            }
        }
    }

    // --- Vertical sweep ---------------------------------------------------
    let dy = body.velocity.y * dt;
    let bottom_before_vertical = body.aabb().min.y;
    if dy != 0.0 {
        let x_span = (body.aabb().min.x, body.aabb().max.x);
        match sweep_axis(
            Axis::Y(dy),
            body.aabb().min.y,
            body.aabb().max.y,
            x_span,
            body.drop_through_grace <= 0.0,
            context,
        ) {
            SweepHit::None => body.position.y += dy,
            SweepHit::Blocked { edge, source } => {
                if dy > 0.0 {
                    // Head-bonk corner correction: when the ceiling overlap
                    // is a shallow sliver, slide around the lip instead of
                    // killing the jump. Mis-timed jumps near ledge corners
                    // feel like the player's fault, not the physics'.
                    let original_x = body.position.x;
                    let mut corrected = false;
                    if body.corner_correction > 0.0 {
                        let mut offset = SKIN + 1.0;
                        while offset <= body.corner_correction {
                            for sign in [-1.0_f32, 1.0_f32] {
                                body.position.x = original_x + sign * offset;
                                let shifted_span = (body.aabb().min.x, body.aabb().max.x);
                                let shifted = sweep_axis(
                                    Axis::Y(dy),
                                    body.aabb().min.y,
                                    body.aabb().max.y,
                                    shifted_span,
                                    body.drop_through_grace <= 0.0,
                                    context,
                                );
                                if matches!(shifted, SweepHit::None) {
                                    corrected = true;
                                    break;
                                }
                            }
                            if corrected {
                                break;
                            }
                            body.position.x = original_x;
                            offset += 2.0;
                        }
                    }
                    if corrected {
                        body.position.y += dy;
                    } else {
                        body.position.x = original_x;
                        body.position.y = edge - body.half_size.y - SKIN;
                        body.velocity.y = 0.0;
                        body.on_ceiling = true;
                        events.push(CollisionEvent {
                            side: ContactSide::Ceiling,
                            surface: contact_surface(source),
                        });
                        collided = true;
                    }
                } else {
                    body.position.y = edge + body.half_size.y + SKIN;
                    if body.velocity.y < 0.0 {
                        body.velocity.y = 0.0;
                    }
                    body.on_ground = true;
                    floor_seen = true;
                    events.push(CollisionEvent {
                        side: ContactSide::Floor,
                        surface: contact_surface(source),
                    });
                    // Only genuine platforms confer riding.
                    if let HitSource::Platform(index) = source {
                        riding = Some(index);
                    }
                    collided = true;
                }
            }
        }
    } else {
        // Zero-descent frames still renew floor contact so resting bodies do
        // not flicker between grounded and airborne.
        if let Some((surface, platform_index)) = ground_probe(body, context, SKIN * 2.0) {
            let _ = surface;
            body.on_ground = true;
            body.riding = platform_index;
        }
    }

    // --- Slope surface snap ----------------------------------------------
    // Ramps are not AABBs, so sweeps cannot resolve them; bodies approaching
    // a ramp surface are snapped onto it — even while flagged grounded by a
    // coincident AABB edge at ramp junctions, which is exactly where walk-up
    // ramps meet platforms.
    if !context.slopes.is_empty() {
        let x_span = (body.aabb().min.x, body.aabb().max.x);
        for slope in context.slopes {
            if !spans_overlap(slope.bounds.min.x, slope.bounds.max.x, x_span) {
                continue;
            }
            let surface = slope.surface_at(
                body.position
                    .x
                    .clamp(slope.bounds.min.x, slope.bounds.max.x),
            );
            let bottom = body.aabb().min.y;
            // Falling from above (crossing inside this tick) or within a
            // travel-scaled band. Climbing (surface above the feet) gets a
            // wider allowance so a walk-up cannot be zipper-stuck at flush
            // ramp-to-platform junctions; descending stays tight to avoid
            // yanking bodies off neighboring ledges onto lower ramps.
            let ratio = slope.slope_ratio().abs();
            let climb_allowance = SLOPE_SNAP * 2.0 + dx.abs() * ratio * 3.0;
            let descent_allowance = SLOPE_SNAP + dx.abs() * ratio;
            let crossed =
                bottom_before_vertical >= surface && bottom <= surface && body.velocity.y <= 0.0;
            let near = body.velocity.y <= 0.0
                && if surface > bottom {
                    surface - bottom <= climb_allowance
                } else {
                    bottom - surface <= descent_allowance
                };
            if crossed || near {
                body.position.y = surface + body.half_size.y + SKIN;
                if body.velocity.y < 0.0 {
                    body.velocity.y = 0.0;
                }
                body.on_ground = true;
                if !floor_seen {
                    events.push(CollisionEvent {
                        side: ContactSide::Floor,
                        surface: ContactSurface::Slope,
                    });
                }
                break;
            }
        }
    }

    if riding.is_some() {
        body.riding = riding;
    }
    collided
}

/// Convenience wrapper that runs a [`CharacterParams`] controller and a
/// physics step together: intent in, moved body out.
pub fn step_character(
    body: &mut KinematicBody,
    controller: &mut CharacterParams,
    intent: Intent,
    gravity: f32,
    delta_seconds: f32,
    context: &CollisionContext<'_>,
) -> bool {
    controller.apply(body, intent, gravity, delta_seconds);
    physics_step(body, gravity, delta_seconds, context)
}

#[derive(Clone, Copy)]
enum Axis {
    X(f32),
    Y(f32),
}

/// Which collider list produced a blocking edge; platforms additionally
/// confer riding on floor hits.
#[derive(Clone, Copy)]
enum HitSource {
    World,
    Platform(usize),
    OneWay,
}

/// Public side of a blocking hit source for the event stream.
fn contact_surface(source: HitSource) -> ContactSurface {
    match source {
        HitSource::World => ContactSurface::World,
        HitSource::Platform(index) => ContactSurface::Platform(index),
        HitSource::OneWay => ContactSurface::OneWay,
    }
}

/// Wall contact side for a horizontal sweep traveling `delta_x`.
fn wall_side(delta_x: f32) -> ContactSide {
    if delta_x > 0.0 {
        ContactSide::WallRight
    } else {
        ContactSide::WallLeft
    }
}

enum SweepHit {
    None,
    Blocked { edge: f32, source: HitSource },
}

/// Unified per-axis sweep: gathers the nearest blocking plane in the axis'
/// travel direction from every compatible collider.
///
/// One-way ledges are considered only on downward travel (`Axis::Y(negative)`)
/// while no drop-through grace remains.
fn sweep_axis(
    axis: Axis,
    low: f32,
    high: f32,
    cross_span: (f32, f32),
    one_way_active: bool,
    context: &CollisionContext<'_>,
) -> SweepHit {
    let delta = match axis {
        Axis::X(dx) => dx,
        Axis::Y(dy) => dy,
    };
    if delta == 0.0 || !delta.is_finite() {
        return SweepHit::None;
    }

    // Swept region covering both the original and moved extents on the axis,
    // exact on the cross axis.
    let (travel_low, travel_high) = if delta > 0.0 {
        (low, high + delta)
    } else {
        (low + delta, high)
    };

    let mut blocked_edge: Option<(f32, HitSource)> = None;
    let mut consider = |edge: f32, source: HitSource| {
        let better = match blocked_edge {
            None => true,
            Some((current, _)) => {
                if delta > 0.0 {
                    edge < current
                } else {
                    edge > current
                }
            }
        };
        if better {
            blocked_edge = Some((edge, source));
        }
    };

    for solid in context.solids {
        if !spans_overlap(low_of(solid, &axis), high_of(solid, &axis), cross_span) {
            continue;
        }
        if !region_hits(solid, axis, travel_low, travel_high, cross_span) {
            continue;
        }
        consider(face_toward(solid, &axis, delta), HitSource::World);
    }
    for (index, platform) in context.platforms.iter().enumerate() {
        let bounds = &platform.bounds;
        if !spans_overlap(low_of(bounds, &axis), high_of(bounds, &axis), cross_span) {
            continue;
        }
        if !region_hits(bounds, axis, travel_low, travel_high, cross_span) {
            continue;
        }
        consider(
            face_toward(bounds, &axis, delta),
            HitSource::Platform(index),
        );
    }
    // One-way ledges participate only on downward travel while no
    // drop-through grace remains.
    if let (Axis::Y(dy), true) = (&axis, one_way_active) {
        if *dy < 0.0 {
            // Crossing rule: the body's underside must begin at or above the
            // ledge surface before this step.
            let descent_start = low;
            for ledge in context.one_ways {
                if !spans_overlap(ledge.min.x, ledge.max.x, cross_span) {
                    continue;
                }
                if descent_start < ledge.max.y - ONE_WAY_CROSS {
                    continue;
                }
                let surface = ledge.max.y;
                if surface <= descent_start + ONE_WAY_CROSS && surface >= travel_low - ONE_WAY_CROSS
                {
                    consider(surface, HitSource::OneWay);
                }
            }
        }
    }
    if let Some(map) = context.tilemap {
        let probe = region_bounds(axis, travel_low, travel_high, cross_span);
        for cell in map.solid_cells_intersecting(probe) {
            let Some(bounds) = map.cell_bounds(cell) else {
                continue;
            };
            consider(face_toward(&bounds, &axis, delta), HitSource::World);
        }
    }

    match blocked_edge {
        None => SweepHit::None,
        Some((edge, source)) => SweepHit::Blocked { edge, source },
    }
}

fn low_of(rect: &Aabb, axis: &Axis) -> f32 {
    match axis {
        Axis::X(_) => rect.min.y,
        Axis::Y(_) => rect.min.x,
    }
}

fn high_of(rect: &Aabb, axis: &Axis) -> f32 {
    match axis {
        Axis::X(_) => rect.max.y,
        Axis::Y(_) => rect.max.x,
    }
}

fn spans_overlap(a_low: f32, a_high: f32, b: (f32, f32)) -> bool {
    a_low <= b.1 && a_high >= b.0
}

fn region_bounds(axis: Axis, travel_low: f32, travel_high: f32, cross_span: (f32, f32)) -> Aabb {
    match axis {
        Axis::X(_) => Aabb::new(
            Vec2::new(travel_low, cross_span.0),
            Vec2::new(travel_high, cross_span.1),
        ),
        Axis::Y(_) => Aabb::new(
            Vec2::new(cross_span.0, travel_low),
            Vec2::new(cross_span.1, travel_high),
        ),
    }
}

fn region_hits(
    rect: &Aabb,
    axis: Axis,
    travel_low: f32,
    travel_high: f32,
    cross_span: (f32, f32),
) -> bool {
    let rect_span = (low_of(rect, &axis), high_of(rect, &axis));
    if !spans_overlap(rect_span.0, rect_span.1, cross_span) {
        return false;
    }
    let axis_low = match axis {
        Axis::X(_) => rect.min.x,
        Axis::Y(_) => rect.min.y,
    };
    let axis_high = match axis {
        Axis::X(_) => rect.max.x,
        Axis::Y(_) => rect.max.y,
    };
    axis_low <= travel_high && axis_high >= travel_low
}

fn face_toward(rect: &Aabb, axis: &Axis, delta: f32) -> f32 {
    match (axis, delta > 0.0) {
        (Axis::X(_), true) => rect.min.x,
        (Axis::X(_), false) => rect.max.x,
        (Axis::Y(_), true) => rect.min.y,
        (Axis::Y(_), false) => rect.max.y,
    }
}

/// Thin-band search directly beneath a body. Returns the highest supporting
/// surface and (when applicable) the platform index providing it.
pub fn ground_probe(
    body: &KinematicBody,
    context: &CollisionContext<'_>,
    depth: f32,
) -> Option<(f32, Option<usize>)> {
    let aabb = body.aabb();
    let band_min_y = aabb.min.y - depth;
    let band = Aabb::new(
        Vec2::new(aabb.min.x, band_min_y),
        Vec2::new(aabb.max.x, aabb.min.y - SKIN),
    );
    let mut best: Option<(f32, Option<usize>)> = None;

    for solid in context.solids {
        if solid.intersects(band) && solid.max.y > band_max(best) {
            best = Some((solid.max.y, None));
        }
    }
    for (index, platform) in context.platforms.iter().enumerate() {
        if platform.bounds.intersects(band) && platform.bounds.max.y > band_max(best) {
            best = Some((platform.bounds.max.y, Some(index)));
        }
    }
    if body.drop_through_grace <= 0.0 {
        for ledge in context.one_ways {
            if ledge.intersects(band) && ledge.max.y > band_max(best) {
                best = Some((ledge.max.y, None));
            }
        }
    }
    if let Some(map) = context.tilemap {
        for cell in map.solid_cells_intersecting(band) {
            let Some(bounds) = map.cell_bounds(cell) else {
                continue;
            };
            if bounds.max.y > band_max(best) {
                best = Some((bounds.max.y, None));
            }
        }
    }
    // Ramps: support with the surface height at the body's clamped center.
    for slope in context.slopes {
        if !spans_overlap(
            slope.bounds.min.x,
            slope.bounds.max.x,
            (aabb.min.x, aabb.max.x),
        ) {
            continue;
        }
        let surface = slope.surface_at(
            aabb.center()
                .x
                .clamp(slope.bounds.min.x, slope.bounds.max.x),
        );
        if surface >= band_min_y && surface <= aabb.min.y - SKIN && surface > band_max(best) {
            best = Some((surface, None));
        }
    }
    best
}

fn band_max(candidate: Option<(f32, Option<usize>)>) -> f32 {
    candidate.map_or(f32::NEG_INFINITY, |(surface, _)| surface)
}

/// Player-facing controller tuning plus its private timing state.
///
/// Every field is public so games can tune feel at runtime; the timers at the
/// bottom are engine-managed bookkeeping copied with `Clone` for free.
#[derive(Debug, Clone)]
pub struct CharacterParams {
    /// Ground run speed cap in world units per second.
    pub run_speed: f32,
    /// Ground acceleration toward run speed (units/s²).
    pub ground_accel: f32,
    /// Ground deceleration toward zero when no input (units/s²).
    pub ground_decel: f32,
    /// Airborne acceleration (units/s²).
    pub air_accel: f32,
    /// Initial upward velocity on jump (units/s).
    pub jump_velocity: f32,
    /// Seconds after leaving ground a jump press still fires (ledge mercy).
    pub coyote_time: f32,
    /// Seconds a jump press is remembered for landing/apex commitment.
    pub jump_buffer_time: f32,
    /// Fraction of upward velocity kept when the jump key releases early.
    pub jump_cut_multiplier: f32,
    /// |vy| below which the apex gravity scale applies (units/s).
    pub apex_threshold: f32,
    /// Gravity multiplier near zero vertical speed (hang floatiness).
    pub apex_gravity_scale: f32,
    /// Gravity multiplier while falling (game-feel snap).
    pub fall_gravity_scale: f32,
    /// Maximum downward speed (terminal velocity, units/s).
    pub max_fall_speed: f32,
    /// Maximum sliding speed against a wall (units/s).
    pub wall_slide_speed: f32,
    /// Upward impulse of a wall jump (units/s).
    pub wall_jump_vertical: f32,
    /// Horizontal launch away from the wall (units/s).
    pub wall_jump_horizontal: f32,
    /// Seconds after a wall jump during which input steering is dampened.
    pub wall_jump_control_lock: f32,
    /// Multiplier on [`Self::ground_decel`] while grounded and reversing at
    /// speed. Higher = snappier turnarounds; `1.0` restores the old feel.
    pub skid_turn_multiplier: f32,

    // --- Engine-managed state (clone-safe) --------------------------------
    coyote_remaining: f32,
    jump_buffer_remaining: f32,
    wall_coyote_remaining: f32,
    wall_side_at_ledge: f32,
    control_lock_remaining: f32,
    active_jump_cut_done: bool,
}

impl Default for CharacterParams {
    fn default() -> Self {
        Self {
            run_speed: 260.0,
            ground_accel: 3_600.0,
            ground_decel: 4_200.0,
            air_accel: 2_200.0,
            jump_velocity: 640.0,
            coyote_time: 0.1,
            jump_buffer_time: 0.12,
            jump_cut_multiplier: 0.45,
            apex_threshold: 60.0,
            apex_gravity_scale: 0.55,
            fall_gravity_scale: 1.35,
            max_fall_speed: 900.0,
            wall_slide_speed: 140.0,
            wall_jump_vertical: 600.0,
            wall_jump_horizontal: 340.0,
            wall_jump_control_lock: 0.12,
            skid_turn_multiplier: 2.4,
            coyote_remaining: 0.0,
            jump_buffer_remaining: 0.0,
            wall_coyote_remaining: 0.0,
            wall_side_at_ledge: 0.0,
            control_lock_remaining: 0.0,
            active_jump_cut_done: false,
        }
    }
}

/// Per-frame player intent consumed by [`step_character`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Intent {
    /// Horizontal input in [-1, 1]; sign maps to facing.
    pub move_x: f32,
    /// Jump pressed this exact frame (edge, not level).
    pub jump_pressed: bool,
    /// Jump currently held (drives variable height).
    pub jump_held: bool,
}

impl CharacterParams {
    /// True while post-wall-jump steering is still dampened. Games can use
    /// this to gate facing flips or animation states during the lock.
    pub fn steering_locked(&self) -> bool {
        self.control_lock_remaining > 0.0
    }

    /// Disarms the variable-jump cut for the current ascent. Stomp and
    /// bounce impulses set by the game are not player jumps, so releasing
    /// the jump button mid-bounce must not trim them.
    pub fn suppress_next_jump_cut(&mut self) {
        self.active_jump_cut_done = true;
    }

    /// Feeds intent into the body's velocity ahead of [`physics_step`].
    ///
    /// Gravity is *not* integrated here; [`physics_step`] applies it using
    /// this controller's apex/fall shaping through `body.gravity_scale`.
    pub fn apply(
        &mut self,
        body: &mut KinematicBody,
        intent: Intent,
        _gravity: f32,
        delta_seconds: f32,
    ) {
        let dt = delta_seconds.max(0.0);

        // Tick mercy windows down first.
        self.coyote_remaining = (self.coyote_remaining - dt).max(0.0);
        self.wall_coyote_remaining = (self.wall_coyote_remaining - dt).max(0.0);
        self.control_lock_remaining = (self.control_lock_remaining - dt).max(0.0);
        if intent.jump_pressed {
            self.jump_buffer_remaining = self.jump_buffer_time;
        } else {
            self.jump_buffer_remaining = (self.jump_buffer_remaining - dt).max(0.0);
        }

        if body.on_ground {
            self.coyote_remaining = self.coyote_time;
            self.active_jump_cut_done = false;
        }
        let wall_touch = if body.on_wall_left {
            Some(-1.0)
        } else if body.on_wall_right {
            Some(1.0)
        } else {
            None
        };
        if let Some(side) = wall_touch {
            self.wall_coyote_remaining = self.coyote_time;
            self.wall_side_at_ledge = side;
        }

        // Commit to whichever jump opportunity exists: floor/coyote first,
        // then wall proximity (including the short post-leave window).
        let grounded_jump = self.jump_buffer_remaining > 0.0 && self.coyote_remaining > 0.0;
        let wall_jump_ready = self.jump_buffer_remaining > 0.0 && self.wall_coyote_remaining > 0.0;
        if grounded_jump {
            body.velocity.y = self.jump_velocity;
            self.jump_buffer_remaining = 0.0;
            self.coyote_remaining = 0.0;
            self.active_jump_cut_done = false;
        } else if wall_jump_ready {
            let away = -self.wall_side_at_ledge.signum();
            body.velocity.y = self.wall_jump_vertical;
            body.velocity.x = away * self.wall_jump_horizontal;
            self.control_lock_remaining = self.wall_jump_control_lock;
            self.jump_buffer_remaining = 0.0;
            self.wall_coyote_remaining = 0.0;
            self.active_jump_cut_done = false;
        }

        // Variable jump height: releasing early trims remaining ascent once.
        if !intent.jump_held && body.velocity.y > 0.0 && !self.active_jump_cut_done {
            body.velocity.y *= self.jump_cut_multiplier;
            self.active_jump_cut_done = true;
        }

        // --- Horizontal steering ----------------------------------------
        let steer_lock = if self.control_lock_remaining > 0.0 {
            0.15
        } else {
            1.0
        };
        let target_speed = intent.move_x.clamp(-1.0, 1.0) * self.run_speed * steer_lock;
        // Reversing at speed brakes harder than neutral stopping: turnaround
        // feel is the single biggest "controls are responsive" tell.
        let reversing = body.on_ground
            && target_speed.abs() > f32::EPSILON
            && body.velocity.x.abs() > 60.0
            && body.velocity.x.signum() == -target_speed.signum();
        let rate = if body.on_ground {
            if target_speed.abs() > f32::EPSILON {
                if reversing {
                    self.ground_decel * self.skid_turn_multiplier
                } else {
                    self.ground_accel
                }
            } else {
                self.ground_decel
            }
        } else {
            self.air_accel
        };
        body.velocity.x = approach(body.velocity.x, target_speed, rate * dt);

        // Wall slide caps descent while pressing into the touched wall.
        // Inclusive comparison keeps the cap sticky: at exactly the cap
        // speed gravity must not re-engage, or sliding oscillates.
        let mut wall_sliding = false;
        if let Some(side) = wall_touch {
            let pressing_in = intent.move_x.signum() == side && intent.move_x != 0.0;
            if pressing_in && body.velocity.y <= -self.wall_slide_speed {
                body.velocity.y = -self.wall_slide_speed;
                wall_sliding = true;
            }
        }

        // Terminal velocity, then shape gravity for apex/fall phases by
        // storing the multiplier on the body for physics_step to consume.
        body.velocity.y = body.velocity.y.max(-self.max_fall_speed);
        let abs_vy = body.velocity.y.abs();
        let shaped = if wall_sliding {
            0.0
        } else if abs_vy <= self.apex_threshold {
            self.apex_gravity_scale
        } else if body.velocity.y < 0.0 {
            self.fall_gravity_scale
        } else {
            1.0
        };
        body.gravity_scale = shaped;
    }
}

fn approach(current: f32, target: f32, max_step: f32) -> f32 {
    let delta = target - current;
    if delta.abs() <= max_step {
        target
    } else {
        current + delta.signum() * max_step
    }
}

/// Result of [`raycast_any`]: contact point plus outward normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    pub point: Vec2,
    pub normal: Vec2,
}

/// Marches a ray from `origin` along `direction` (its length is the range).
///
/// Sampling-based like `NavGrid::segment_blocked`: deterministic and cheap,
/// intended for gameplay probes (ground checks, ledge grabs) rather than
/// ballistic precision. Returns the first hit at or before full range.
pub fn raycast_any(
    context: &CollisionContext<'_>,
    origin: Vec2,
    direction: Vec2,
) -> Option<RayHit> {
    let length = direction.length();
    if length <= f32::EPSILON || !length.is_finite() {
        return None;
    }
    let step_length = 2.0_f32.min(length / 8.0);
    let steps = (length / step_length).ceil() as u32;
    let unit = direction / length;

    let hits_rect = |point: Vec2| -> Option<Vec2> {
        for solid in context.solids {
            if solid.contains_point(point) {
                return Some(solid_normal(solid, point));
            }
        }
        for platform in context.platforms {
            if platform.bounds.contains_point(point) {
                return Some(solid_normal(&platform.bounds, point));
            }
        }
        for ledge in context.one_ways {
            if ledge.contains_point(point) {
                return Some(Vec2::Y);
            }
        }
        // Ramps: a hit is a sample within the footprint whose y is within a
        // half-step of the surface line. Returns the slope normal.
        if !context.slopes.is_empty() {
            for slope in context.slopes {
                if point.x < slope.bounds.min.x || point.x > slope.bounds.max.x {
                    continue;
                }
                let surface = slope.surface_at(point.x);
                if (point.y - surface).abs() < 1.0 {
                    return Some(slope_normal(slope));
                }
            }
        }
        if let Some(map) = context.tilemap {
            let cell = map.world_to_cell(point)?;
            let bounds = map.cell_bounds(cell)?;
            if map.is_solid(cell) {
                return Some(solid_normal(&bounds, point));
            }
        }
        None
    };

    for step in 0..=steps {
        let point = origin + unit * (step as f32 * step_length);
        if let Some(normal) = hits_rect(point) {
            return Some(RayHit { point, normal });
        }
    }
    None
}

fn solid_normal(rect: &Aabb, point: Vec2) -> Vec2 {
    // Distance to each face; smallest wins. Handles corner samples sanely by
    // tie-breaking upward (the common walkable case).
    let left = point.x - rect.min.x;
    let right = rect.max.x - point.x;
    let bottom = point.y - rect.min.y;
    let top = rect.max.y - point.y;
    let min = left.min(right).min(bottom.min(top));
    if min == top {
        Vec2::Y
    } else if min == bottom {
        -Vec2::Y
    } else if min == left {
        -Vec2::X
    } else {
        Vec2::X
    }
}

/// Upward-facing normal of a ramp, correct for walkable (≤45°) slopes:
/// `(dy/dx, 1)` normalized points away from the surface.
fn slope_normal(slope: &Slope) -> Vec2 {
    let ratio = slope.slope_ratio();
    Vec2::new(-ratio, 1.0).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::IVec2;

    const GRAVITY: f32 = 1_800.0;
    const DT: f32 = 1.0 / 60.0;

    fn ground_solids() -> Vec<Aabb> {
        vec![Aabb::new(Vec2::new(-500.0, -100.0), Vec2::new(500.0, 0.0))]
    }

    #[test]
    fn body_falls_and_lands_resting_on_floor() {
        let mut body = KinematicBody::new(Vec2::ZERO, Vec2::splat(32.0));
        let solids = vec![Aabb::new(Vec2::new(-100.0, -50.0), Vec2::new(100.0, 0.0))];
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };

        for _ in 0..120 {
            physics_step(&mut body, GRAVITY, DT, &context);
        }

        assert!(body.on_ground);
        assert_eq!(body.velocity.y, 0.0);
        // Resting just above the surface, never inside it.
        assert!(body.aabb().min.y >= 0.0);
        assert!(body.aabb().min.y < 1.0);
    }

    #[test]
    fn fast_bodies_cannot_tunnel_through_thin_floors() {
        let mut body = KinematicBody::new(Vec2::new(0.0, 200.0), Vec2::splat(16.0));
        let solids = vec![Aabb::new(Vec2::new(-100.0, -4.0), Vec2::new(100.0, 0.0))];
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };
        for _ in 0..60 {
            physics_step(&mut body, GRAVITY, DT, &context);
        }
        assert!(body.on_ground);
        assert!(body.aabb().min.y >= 0.0 - f32::EPSILON);
    }

    #[test]
    fn horizontal_sweep_blocks_at_walls_and_reports_side() {
        let mut body = KinematicBody::new(Vec2::new(0.0, 20.0), Vec2::splat(16.0));
        let solids = vec![Aabb::new(Vec2::new(80.0, -50.0), Vec2::new(400.0, 120.0))];
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };
        for _ in 0..90 {
            body.velocity.x = 300.0; // keep driving right into the wall
            physics_step(&mut body, 0.0, DT, &context);
        }
        assert!(body.on_wall_right);
        assert!(!body.on_wall_left);
        assert!(body.aabb().max.x <= 80.0 + SKIN * 2.0);
        assert!(body.velocity.x == 0.0 || body.aabb().max.x < 81.0);

        // And from the other side.
        let mut left_body = KinematicBody::new(Vec2::new(600.0, 20.0), Vec2::splat(16.0));
        for _ in 0..90 {
            left_body.velocity.x = -300.0;
            physics_step(&mut left_body, 0.0, DT, &context);
        }
        assert!(left_body.on_wall_left);
        assert!(left_body.aabb().min.x >= 400.0 - SKIN * 2.0);
    }

    #[test]
    fn ascending_bodies_clamp_against_ceilings() {
        let mut body = KinematicBody::new(Vec2::new(0.0, 40.0), Vec2::splat(32.0));
        let solids = vec![
            Aabb::new(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 20.0)), // floor
            Aabb::new(Vec2::new(-100.0, 120.0), Vec2::new(100.0, 160.0)), // ceiling
        ];
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };
        body.velocity.y = 5_000.0;
        physics_step(&mut body, 0.0, DT, &context);
        assert!(body.on_ceiling);
        assert_eq!(body.velocity.y, 0.0);
        assert!(body.aabb().max.y <= 121.0);
    }

    #[test]
    fn one_way_ledges_catch_descent_but_never_ascent() {
        let ledge = Aabb::new(Vec2::new(-80.0, 100.0), Vec2::new(80.0, 112.0));
        let context_probe = CollisionContext {
            one_ways: &[ledge],
            ..CollisionContext::empty()
        };

        // Falling onto the ledge catches the body.
        let mut falling = KinematicBody::new(Vec2::new(0.0, 140.0), Vec2::splat(16.0));
        for _ in 0..60 {
            physics_step(&mut falling, GRAVITY, DT, &context_probe);
        }
        assert!(falling.on_ground);
        assert!((falling.aabb().min.y - 112.0).abs() < 1.0);

        // Jumping up through it from below is never blocked.
        let mut riser = KinematicBody::new(Vec2::new(0.0, 60.0), Vec2::splat(16.0));
        riser.velocity.y = 800.0;
        let blocked = physics_step(&mut riser, GRAVITY, DT, &context_probe);
        assert!(!blocked);
        assert!(riser.position.y > 60.0);
        assert!(!riser.on_ceiling);
    }

    #[test]
    fn drop_through_grace_passes_through_one_way_tops() {
        let ledge = Aabb::new(Vec2::new(-80.0, 96.0), Vec2::new(80.0, 108.0));
        let basement = vec![Aabb::new(Vec2::new(-200.0, -40.0), Vec2::new(200.0, 0.0))];
        let context = CollisionContext {
            solids: &basement,
            one_ways: &[ledge],
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(0.0, 124.0), Vec2::splat(16.0));
        body.request_drop_through();
        for _ in 0..120 {
            physics_step(&mut body, GRAVITY, DT, &context);
        }
        assert!(
            body.position.y < 90.0,
            "body must fall straight past the ledge, not land on it"
        );
        assert!(body.on_ground, "and settle on the solid floor underneath");
        assert!(body.aabb().min.y < 1.0, "resting on the lower floor");
    }

    #[test]
    fn tilemap_solids_block_both_axes() {
        let mut map = TileMap::new(8, 8, Vec2::splat(32.0));
        // Solid ground row across y=2.
        for x in 0..8 {
            map.set_solid(IVec2::new(x, 2), true);
        }
        // Solid wall column at x=5 above the floor.
        for y in 3..6 {
            map.set_solid(IVec2::new(5, y), true);
        }

        let context = CollisionContext {
            tilemap: Some(&map),
            ..CollisionContext::empty()
        };

        let mut walker = KinematicBody::new(Vec2::new(48.0, 132.0), Vec2::splat(24.0));
        for _ in 0..90 {
            walker.velocity.x = 240.0;
            physics_step(&mut walker, GRAVITY, DT, &context);
        }
        assert!(walker.on_ground);
        assert!(walker.on_wall_right, "the x=5 column must stop the walker");
        // Floor top surface is at y = 3*32 = 96.
        assert!((walker.aabb().min.y - 96.0).abs() < 1.0);
    }

    #[test]
    fn moving_platforms_carry_riders_between_ticks() {
        let initial = Aabb::new(Vec2::new(-64.0, 0.0), Vec2::new(64.0, 16.0));
        let moved = Aabb::new(Vec2::new(36.0, 0.0), Vec2::new(164.0, 16.0));
        let carrying_tick = [Platform {
            bounds: moved,
            delta: Vec2::new(100.0, 0.0),
        }];
        let riding_tick = [Platform {
            bounds: moved,
            delta: Vec2::ZERO,
        }];
        let carry_context = CollisionContext {
            platforms: &carrying_tick,
            ..CollisionContext::empty()
        };

        // Start standing on the platform's original surface.
        let mut rider = KinematicBody::new(Vec2::new(0.0, 16.0 + 12.0 + SKIN), Vec2::splat(24.0));
        rider.riding = Some(0);

        // Tick 1: the game reports the platform jumped +100 X; the rider is
        // carried along before its own integration runs.
        physics_step(&mut rider, GRAVITY, DT, &carry_context);
        assert!(
            (rider.position.x - 100.0).abs() < 0.5,
            "rider carried by platform delta, got {}",
            rider.position.x
        );

        // Subsequent ticks ride the post-move surface in place.
        let ride_context = CollisionContext {
            platforms: &riding_tick,
            ..CollisionContext::empty()
        };
        let _ = initial;
        for _ in 0..6 {
            physics_step(&mut rider, GRAVITY, DT, &ride_context);
        }
        assert!(rider.on_ground);
        assert_eq!(rider.riding, Some(0));
        assert!(rider.aabb().min.y > 14.0 && rider.aabb().min.y < 18.0);
    }

    #[test]
    fn character_jumps_only_once_per_press_even_while_held() {
        let solids = ground_solids();
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::ZERO, Vec2::splat(28.0));
        let mut controller = CharacterParams::default();

        // Settle on the floor.
        for _ in 0..40 {
            step_character(
                &mut body,
                &mut controller,
                Intent::default(),
                GRAVITY,
                DT,
                &context,
            );
        }
        assert!(body.on_ground);

        // Press jump once; keep HOLDING afterward (edges, not levels).
        let mut apexes = 0;
        let mut was_rising = false;
        let mut left_ground_steps = 0;
        for frame in 0..120 {
            let intent = Intent {
                move_x: 0.0,
                jump_pressed: frame == 0,
                jump_held: true,
            };
            step_character(&mut body, &mut controller, intent, GRAVITY, DT, &context);
            if !body.on_ground {
                left_ground_steps += 1;
            }
            if body.velocity.y > 0.0 {
                was_rising = true;
            } else if was_rising {
                apexes += 1;
                was_rising = false;
            }
        }
        assert_eq!(apexes, 1, "one press must produce exactly one jump arc");
        assert!(left_ground_steps > 10, "the jump must leave the ground");
    }

    #[test]
    fn releasing_jump_early_trims_the_arc() {
        let solids = ground_solids();
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };

        let run_height = |hold_frames: usize| -> f32 {
            let mut body = KinematicBody::new(Vec2::ZERO, Vec2::splat(28.0));
            let mut controller = CharacterParams::default();
            for _ in 0..40 {
                step_character(
                    &mut body,
                    &mut controller,
                    Intent::default(),
                    GRAVITY,
                    DT,
                    &context,
                );
            }
            let mut peak = body.position.y;
            for frame in 0..90 {
                let held = frame < hold_frames;
                let intent = Intent {
                    jump_pressed: frame == 0,
                    jump_held: held,
                    ..Intent::default()
                };
                step_character(&mut body, &mut controller, intent, GRAVITY, DT, &context);
                peak = peak.max(body.position.y);
            }
            peak
        };
        let short_hop = run_height(3);
        let full_jump = run_height(45);
        assert!(
            short_hop < full_jump - 40.0,
            "release-cut jumps must be meaningfully shorter ({short_hop} vs {full_jump})"
        );
    }

    #[test]
    fn coyote_time_allows_the_first_frames_after_walking_off() {
        let empty: Vec<Aabb> = Vec::new();
        let airborne = CollisionContext {
            solids: &empty,
            ..CollisionContext::empty()
        };

        let mut body = KinematicBody::new(Vec2::new(0.0, 15.0 + SKIN), Vec2::splat(30.0));
        // Stand on an invisible reference plane to arm ground state.
        let ledge = vec![Aabb::new(Vec2::new(-100.0, -50.0), Vec2::new(100.0, 15.0))];
        let on_platform = CollisionContext {
            solids: &ledge,
            ..CollisionContext::empty()
        };
        let mut controller = CharacterParams::default();
        for _ in 0..30 {
            step_character(
                &mut body,
                &mut controller,
                Intent::default(),
                GRAVITY,
                DT,
                &on_platform,
            );
        }
        assert!(body.on_ground);

        // Walk off and wait a couple of frames (within coyote window).
        for _ in 0..3 {
            step_character(
                &mut body,
                &mut controller,
                Intent::default(),
                GRAVITY,
                DT,
                &airborne,
            );
        }
        assert!(!body.on_ground);
        let height_before = body.position.y;

        // Pressing jump now should still work thanks to coyote time.
        step_character(
            &mut body,
            &mut controller,
            Intent {
                jump_pressed: true,
                ..Intent::default()
            },
            GRAVITY,
            DT,
            &airborne,
        );
        assert!(
            body.velocity.y > 0.0 || body.position.y > height_before,
            "coyote window must permit the jump after leaving the ledge"
        );
    }

    #[test]
    fn wall_slide_caps_falling_speed_and_wall_jumps_launch_away() {
        let walls = vec![Aabb::new(Vec2::new(64.0, -500.0), Vec2::new(200.0, 500.0))];
        let context = CollisionContext {
            solids: &walls,
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(48.0, 200.0), Vec2::splat(16.0));
        let mut controller = CharacterParams::default();

        // Press right into the wall; track descent only once wall contact
        // exists (the initial pre-contact plunge is plain gravity).
        let mut min_vy_during_slide: f32 = 0.0;
        let mut touched = false;
        for _ in 0..90 {
            let intent = Intent {
                move_x: 1.0,
                ..Intent::default()
            };
            step_character(&mut body, &mut controller, intent, GRAVITY, DT, &context);
            if body.on_wall_right {
                touched = true;
            }
            if touched {
                min_vy_during_slide = min_vy_during_slide.min(body.velocity.y);
            }
        }
        assert!(touched, "the body must be pinned against the wall");
        assert!(
            min_vy_during_slide >= -controller.wall_slide_speed - 10.0,
            "slide speed must clamp ({min_vy_during_slide})"
        );

        // Buffered jump against the wall launches up and away.
        let vx_before = body.velocity.x;
        step_character(
            &mut body,
            &mut controller,
            Intent {
                jump_pressed: true,
                ..Intent::default()
            },
            GRAVITY,
            DT,
            &context,
        );
        assert!(body.velocity.y > 200.0, "wall jump must launch upward");
        assert!(
            body.velocity.x < vx_before.min(0.0) || body.velocity.x < -100.0,
            "wall jump must push away from the wall"
        );
    }

    #[test]
    fn raycast_returns_surface_normal_for_probes() {
        let rects = vec![Aabb::new(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 20.0))];
        let floor_top = rects[0].max.y;
        let context = CollisionContext {
            solids: &rects,
            ..CollisionContext::empty()
        };

        let down = raycast_any(&context, Vec2::new(0.0, 200.0), Vec2::new(0.0, -400.0));
        assert!(down.is_some());
        let hit = down.unwrap();
        assert_eq!(hit.normal, Vec2::Y);
        assert!((hit.point.y - floor_top).abs() <= 2.5);

        // Into open sky: no hit.
        assert!(raycast_any(&context, Vec2::new(0.0, 200.0), Vec2::new(0.0, 100.0)).is_none());
    }

    #[test]
    fn zero_delta_steps_still_renew_ground_contact() {
        let solids = ground_solids();
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(0.0, 16.0 + SKIN), Vec2::splat(32.0));
        body.on_ground = false;
        physics_step(&mut body, 0.0, 0.0, &context);
        assert!(body.on_ground, "idle resting bodies stay grounded");
    }

    /// Perf smoke aligned with `.aurora/PERF_BUDGETS.toml`: 1,000 bodies
    /// settling for 10 simulated seconds must finish comfortably inside a
    /// generous wall-clock bound (the bound exists to catch gross
    /// regressions, not to measure absolute speed).
    #[test]
    fn bodies_walk_up_and_down_ramps() {
        // 45-degree ramp from (0,0) to (300,300).
        let ramp = Slope {
            bounds: Aabb::new(Vec2::new(0.0, -50.0), Vec2::new(300.0, 350.0)),
            surface_left: 0.0,
            surface_right: 300.0,
        };
        let context = CollisionContext {
            slopes: std::slice::from_ref(&ramp),
            ..CollisionContext::empty()
        };

        // Uphill: grounded body advances and its feet track the surface.
        let mut body = KinematicBody::new(Vec2::new(40.0, 40.0 + 16.0), Vec2::splat(32.0));
        body.velocity.x = 260.0;
        let mut climbed = 0.0_f32;
        for _ in 0..90 {
            velocity_step(&mut body, &context);
            climbed = climbed.max(body.position.y);
        }
        assert!(climbed > 180.0, "walked up the ramp (max y {climbed:.1})");

        // Downhill: from the top, moving right keeps feet on the surface.
        let mut down = KinematicBody::new(Vec2::new(240.0, 240.0 + 16.0), Vec2::splat(32.0));
        down.velocity.x = 260.0;
        for _ in 0..120 {
            if down.position.x > 290.0 {
                break;
            }
            velocity_step(&mut down, &context);
        }
        assert!(
            down.position.x > 270.0,
            "descended most of the ramp (x {:.1})",
            down.position.x
        );
        let feet = down.aabb().min.y;
        let surface = ramp.surface_at(down.position.x);
        assert!(
            (feet - surface).abs() < SLOPE_SNAP + 1.0,
            "descending body tracks the ramp (feet {feet:.1}, surface {surface:.1})"
        );
    }

    #[test]
    fn steep_ramps_block_like_walls() {
        // 3:1 rise/run — steeper than the wall threshold.
        let steep = Slope {
            bounds: Aabb::new(Vec2::new(0.0, 0.0), Vec2::new(100.0, 400.0)),
            surface_left: 0.0,
            surface_right: 300.0,
        };
        let context = CollisionContext {
            slopes: std::slice::from_ref(&steep),
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(20.0, 16.0), Vec2::splat(32.0));
        body.velocity.x = 260.0;
        for _ in 0..60 {
            velocity_step(&mut body, &context);
        }
        let feet = body.aabb().min.y;
        assert!(feet < 40.0, "steep ramp blocks the climb (feet {feet:.1})");
        assert!(body.on_wall_right || body.velocity.x <= 0.0);
    }

    #[test]
    fn falling_bodies_land_on_ramps_without_tunneling() {
        let ramp = Slope {
            bounds: Aabb::new(Vec2::new(-400.0, -50.0), Vec2::new(400.0, 300.0)),
            surface_left: 60.0,
            surface_right: 260.0,
        };
        let context = CollisionContext {
            slopes: std::slice::from_ref(&ramp),
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(200.0, 900.0), Vec2::splat(32.0));
        body.velocity.y = -1_200.0; // far beyond one tick's snap band
        for _ in 0..120 {
            velocity_step(&mut body, &context);
            if body.on_ground {
                break;
            }
        }
        assert!(body.on_ground, "fast fall lands on the ramp");
        let feet = body.aabb().min.y;
        let surface = ramp.surface_at(body.position.x);
        assert!(
            (feet - surface).abs() < 2.0,
            "landed on the surface (feet {feet:.1}, surface {surface:.1})"
        );
    }

    #[test]
    fn raycast_and_ground_probe_see_ramps() {
        let ramp = Slope {
            bounds: Aabb::new(Vec2::new(0.0, -50.0), Vec2::new(300.0, 300.0)),
            surface_left: 0.0,
            surface_right: 300.0,
        };
        let context = CollisionContext {
            slopes: std::slice::from_ref(&ramp),
            ..CollisionContext::empty()
        };

        let hit = raycast_any(&context, Vec2::new(150.0, 300.0), Vec2::new(0.0, -160.0));
        assert!(hit.is_some(), "downward ray finds the ramp");
        assert!(
            hit.expect("checked").normal.y > 0.5,
            "slope normal faces mostly upward"
        );

        // Probe directly beneath a body offset from ramp start.
        let body = KinematicBody::new(Vec2::new(150.0, 180.0), Vec2::splat(32.0));
        let (surface, platform) = ground_probe(&body, &context, 24.0).expect("probe digs in");
        assert!(platform.is_none(), "slopes confer no riding");
        assert!(
            (surface - 150.0).abs() < 1.0,
            "probe returns the surface height ({surface:.1})"
        );
    }

    /// One physics tick with a caller-maintained walking velocity: re-applies
    /// `body.velocity.x` intent so tests can drive without a controller.
    fn velocity_step(body: &mut KinematicBody, context: &CollisionContext<'_>) {
        let intent_x = body.velocity.x;
        // Integrate gravity and resolve collisions; then restore the walking
        // velocity (the wall-to-zero reset is what wall behavior should see).
        physics_step(body, GRAVITY, DT, context);
        if body.on_wall_left || body.on_wall_right {
            return; // let wall/ramp blocking stand
        }
        body.velocity.x = intent_x;
    }

    #[test]
    fn dense_worlds_settle_inside_the_perf_budget() {
        use std::time::Instant;

        const BODIES: usize = 1_000;
        const SETTLE_TICKS: u32 = 600;
        // 10 seconds of wall clock for ~10 seconds of simulation across
        // 1k bodies; observed runtime is well under a second on a laptop.
        const BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

        // A shelf world: bodies fall onto alternating platforms.
        let mut solids = vec![Aabb::new(
            Vec2::new(-40_000.0, -100.0),
            Vec2::new(40_000.0, 0.0),
        )];
        for shelf in 0..5 {
            let y = 200.0 + shelf as f32 * 150.0;
            solids.push(Aabb::new(
                Vec2::new(-30_000.0 + shelf as f32 * 900.0, y),
                Vec2::new(-20_000.0 + shelf as f32 * 900.0, y + 40.0),
            ));
        }
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };
        let mut bodies: Vec<KinematicBody> = (0..BODIES)
            .map(|index| {
                let x = (index % 50) as f32 * 40.0 - 1_000.0;
                let y = 400.0 + (index / 50) as f32 * 60.0;
                KinematicBody::new(Vec2::new(x, y), Vec2::splat(28.0))
            })
            .collect();

        let started = Instant::now();
        for _ in 0..SETTLE_TICKS {
            for body in &mut bodies {
                physics_step(body, GRAVITY, DT, &context);
            }
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < BUDGET,
            "1k bodies x 600 steps took {elapsed:?} (budget {BUDGET:?})"
        );
        let grounded = bodies.iter().filter(|body| body.on_ground).count();
        assert!(
            grounded > BODIES * 9 / 10,
            "settled world should ground nearly every body ({grounded}/{BODIES})"
        );
    }

    #[test]
    fn shallow_head_bonks_slide_around_the_corner() {
        // Ceiling whose left edge barely overlaps the jumping body's right
        // half: corner correction should carry the body around the lip.
        let solids = vec![
            Aabb::new(Vec2::new(-500.0, -100.0), Vec2::new(500.0, 0.0)),
            Aabb::new(Vec2::new(10.0, 150.0), Vec2::new(400.0, 250.0)),
        ];
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(0.0, 40.0), Vec2::new(32.0, 32.0));
        body.corner_correction = 10.0;
        body.velocity.y = 640.0;

        let mut bonked = false;
        let mut apex = body.aabb().max.y;
        for _ in 0..40 {
            physics_step(&mut body, GRAVITY, DT, &context);
            apex = apex.max(body.aabb().max.y);
            if body.on_ceiling {
                bonked = true;
                break;
            }
        }
        assert!(
            !bonked,
            "shallow overlap must slide around the lip, not bonk (pos {:?})",
            body.position
        );
        assert!(
            apex > 150.0,
            "body cleared the lip and kept rising (apex {apex:.1})"
        );
    }

    #[test]
    fn deep_head_bonks_still_stop_and_report() {
        let solids = vec![
            Aabb::new(Vec2::new(-500.0, -100.0), Vec2::new(500.0, 0.0)),
            Aabb::new(Vec2::new(-200.0, 150.0), Vec2::new(200.0, 250.0)),
        ];
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(0.0, 40.0), Vec2::new(32.0, 32.0));
        body.velocity.y = 640.0;
        let mut bonked = false;
        for _ in 0..40 {
            physics_step(&mut body, GRAVITY, DT, &context);
            if body.on_ceiling {
                bonked = true;
                break;
            }
        }
        assert!(bonked, "deep overlap is a genuine ceiling");

        let mut body = KinematicBody::new(Vec2::new(0.0, 40.0), Vec2::new(32.0, 32.0));
        body.corner_correction = 0.0;
        body.velocity.y = 640.0;
        let mut bonked = false;
        for _ in 0..40 {
            physics_step(&mut body, GRAVITY, DT, &context);
            if body.on_ceiling {
                bonked = true;
                break;
            }
        }
        assert!(bonked, "corner_correction = 0 restores the hard stop");
    }

    #[test]
    fn grounded_bodies_step_up_small_lips() {
        // Floor with an 8-unit lip in the walk path.
        let solids = vec![
            Aabb::new(Vec2::new(-300.0, -100.0), Vec2::new(100.0, 0.0)),
            Aabb::new(Vec2::new(100.0, -100.0), Vec2::new(500.0, 8.0)),
        ];
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(60.0, 16.0), Vec2::new(32.0, 32.0));
        body.step_height = 12.0;
        for _ in 0..60 {
            let grounded = physics_step(&mut body, GRAVITY, DT, &context);
            let _ = grounded;
            body.velocity.x = 260.0; // keep walking right
        }
        assert!(
            body.position.x > 132.0,
            "walked up onto the lip (x = {:.1})",
            body.position.x
        );
        assert!(
            body.position.y >= 20.0,
            "standing on the raised floor (y = {:.1})",
            body.position.y
        );
        assert!(!body.on_wall_right, "the lip never read as a wall");
    }

    #[test]
    fn tall_walls_are_never_stepped() {
        let solids = vec![
            Aabb::new(Vec2::new(-300.0, -100.0), Vec2::new(100.0, 0.0)),
            Aabb::new(Vec2::new(100.0, -100.0), Vec2::new(500.0, 400.0)),
        ];
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(60.0, 16.0), Vec2::new(32.0, 32.0));
        body.step_height = 12.0;
        for _ in 0..60 {
            physics_step(&mut body, GRAVITY, DT, &context);
            body.velocity.x = 260.0;
        }
        assert!(body.position.x < 84.0 + 1.0, "stopped at the wall");
        assert!(body.on_wall_right, "tall geometry still reports a wall");
    }

    #[test]
    fn reversing_at_speed_brakes_harder_than_neutral_stopping() {
        let solids = ground_solids();
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };
        let run = 260.0;
        let mut with_skid = KinematicBody::new(Vec2::new(0.0, 20.0), Vec2::splat(16.0));
        let mut params_skid = CharacterParams {
            run_speed: run,
            skid_turn_multiplier: 2.4,
            ..CharacterParams::default()
        };
        let mut without_skid = with_skid;
        let mut params_plain = CharacterParams {
            run_speed: run,
            skid_turn_multiplier: 1.0,
            ..CharacterParams::default()
        };

        // Build up full rightward speed first.
        for body in [&mut with_skid, &mut without_skid] {
            let mut params = CharacterParams {
                run_speed: run,
                ..CharacterParams::default()
            };
            for _ in 0..60 {
                params.apply(
                    body,
                    Intent {
                        move_x: 1.0,
                        ..Default::default()
                    },
                    GRAVITY,
                    DT,
                );
                physics_step(body, GRAVITY, DT, &context);
            }
        }
        assert!(with_skid.velocity.x > run * 0.95);

        // Now slam the stick the other way and race. Five ticks separates a
        // 2.4x skid brake from the plain decel curve.
        for _ in 0..5 {
            let intent = Intent {
                move_x: -1.0,
                ..Default::default()
            };
            params_skid.apply(&mut with_skid, intent, GRAVITY, DT);
            physics_step(&mut with_skid, GRAVITY, DT, &context);
            params_plain.apply(&mut without_skid, intent, GRAVITY, DT);
            physics_step(&mut without_skid, GRAVITY, DT, &context);
        }
        assert!(
            with_skid.velocity.x < without_skid.velocity.x - 80.0,
            "skid turn must outrun the plain turnaround (skid {}, plain {})",
            with_skid.velocity.x,
            without_skid.velocity.x
        );
    }

    #[test]
    fn landing_pushes_exactly_one_floor_event_with_the_right_surface() {
        let solids = vec![Aabb::new(Vec2::new(-100.0, -50.0), Vec2::new(100.0, 0.0))];
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(0.0, 80.0), Vec2::splat(16.0));
        let mut events = Vec::new();
        let mut landed = false;
        for _ in 0..120 {
            events.clear();
            physics_step_events(&mut body, GRAVITY, DT, &context, &mut events);
            if body.on_ground {
                landed = true;
                break;
            }
        }
        assert!(landed);
        assert_eq!(
            events,
            vec![CollisionEvent {
                side: ContactSide::Floor,
                surface: ContactSurface::World,
            }]
        );
    }

    #[test]
    fn platform_landings_report_the_platform_index() {
        let platforms = [Platform {
            bounds: Aabb::new(Vec2::new(-100.0, -8.0), Vec2::new(100.0, 0.0)),
            delta: Vec2::ZERO,
        }];
        let context = CollisionContext {
            platforms: &platforms,
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(0.0, 60.0), Vec2::splat(16.0));
        let mut events = Vec::new();
        for _ in 0..120 {
            events.clear();
            physics_step_events(&mut body, GRAVITY, DT, &context, &mut events);
            if body.on_ground {
                break;
            }
        }
        assert!(body.on_ground);
        assert_eq!(
            events,
            vec![CollisionEvent {
                side: ContactSide::Floor,
                surface: ContactSurface::Platform(0),
            }]
        );
    }

    #[test]
    fn one_way_landings_report_the_one_way_surface() {
        let ledge = Aabb::new(Vec2::new(-80.0, 100.0), Vec2::new(80.0, 112.0));
        let context = CollisionContext {
            one_ways: &[ledge],
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(0.0, 140.0), Vec2::splat(16.0));
        let mut events = Vec::new();
        for _ in 0..60 {
            events.clear();
            physics_step_events(&mut body, GRAVITY, DT, &context, &mut events);
            if body.on_ground {
                break;
            }
        }
        assert!(body.on_ground);
        assert_eq!(
            events,
            vec![CollisionEvent {
                side: ContactSide::Floor,
                surface: ContactSurface::OneWay,
            }]
        );
    }

    #[test]
    fn wall_blocks_push_wall_events_for_the_right_side() {
        let solids = vec![Aabb::new(Vec2::new(80.0, -50.0), Vec2::new(400.0, 120.0))];
        let context = CollisionContext {
            solids: &solids,
            ..CollisionContext::empty()
        };

        let mut body = KinematicBody::new(Vec2::new(0.0, 20.0), Vec2::splat(16.0));
        let mut events = Vec::new();
        let mut blocked = false;
        for _ in 0..90 {
            body.velocity.x = 300.0;
            events.clear();
            physics_step_events(&mut body, 0.0, DT, &context, &mut events);
            if body.on_wall_right {
                blocked = true;
                break;
            }
        }
        assert!(blocked);
        assert_eq!(
            events,
            vec![CollisionEvent {
                side: ContactSide::WallRight,
                surface: ContactSurface::World,
            }]
        );

        // Same wall from the far side reports WallLeft.
        let mut left_body = KinematicBody::new(Vec2::new(600.0, 20.0), Vec2::splat(16.0));
        let mut left_events = Vec::new();
        let mut left_blocked = false;
        for _ in 0..90 {
            left_body.velocity.x = -300.0;
            left_events.clear();
            physics_step_events(&mut left_body, 0.0, DT, &context, &mut left_events);
            if left_body.on_wall_left {
                left_blocked = true;
                break;
            }
        }
        assert!(left_blocked);
        assert_eq!(
            left_events,
            vec![CollisionEvent {
                side: ContactSide::WallLeft,
                surface: ContactSurface::World,
            }]
        );
    }

    #[test]
    fn slope_snap_landings_push_floor_with_slope_surface() {
        let ramp = Slope {
            bounds: Aabb::new(Vec2::new(-400.0, -50.0), Vec2::new(400.0, 300.0)),
            surface_left: 60.0,
            surface_right: 260.0,
        };
        let context = CollisionContext {
            slopes: std::slice::from_ref(&ramp),
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(200.0, 900.0), Vec2::splat(32.0));
        body.velocity.y = -1_200.0;
        let mut events = Vec::new();
        let mut landed = false;
        for _ in 0..120 {
            events.clear();
            physics_step_events(&mut body, GRAVITY, DT, &context, &mut events);
            if body.on_ground {
                landed = true;
                break;
            }
        }
        assert!(landed, "fast fall lands on the ramp");
        assert_eq!(
            events,
            vec![CollisionEvent {
                side: ContactSide::Floor,
                surface: ContactSurface::Slope,
            }]
        );
    }

    #[test]
    fn clean_falls_push_no_events() {
        let context = CollisionContext::empty();
        let mut body = KinematicBody::new(Vec2::new(0.0, 500.0), Vec2::splat(16.0));
        let mut events = Vec::new();
        for _ in 0..60 {
            physics_step_events(&mut body, GRAVITY, DT, &context, &mut events);
        }
        assert!(events.is_empty(), "no geometry, no contacts, no events");
        assert!(!body.on_ground);
    }

    #[test]
    fn submerged_bodies_sink_slower_than_bodies_in_air() {
        let water = vec![Aabb::new(
            Vec2::new(-200.0, -400.0),
            Vec2::new(200.0, 200.0),
        )];
        let wet = CollisionContext {
            water: &water,
            ..CollisionContext::empty()
        };
        let dry = CollisionContext::empty();

        let mut sunk = KinematicBody::new(Vec2::new(0.0, 100.0), Vec2::splat(16.0));
        let mut fell = KinematicBody::new(Vec2::new(0.0, 100.0), Vec2::splat(16.0));
        for _ in 0..60 {
            physics_step(&mut sunk, GRAVITY, DT, &wet);
            physics_step(&mut fell, GRAVITY, DT, &dry);
        }
        assert!(sunk.in_water);
        assert!(!fell.in_water);
        assert!(
            sunk.position.y > fell.position.y,
            "water must slow the descent (wet {}, dry {})",
            sunk.position.y,
            fell.position.y
        );
    }

    #[test]
    fn water_drag_decays_velocity_while_submerged() {
        let water = vec![Aabb::new(
            Vec2::new(-200.0, -400.0),
            Vec2::new(200.0, 200.0),
        )];
        let context = CollisionContext {
            water: &water,
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(0.0, 100.0), Vec2::splat(16.0));
        body.velocity = Vec2::new(200.0, -100.0);
        physics_step(&mut body, 0.0, DT, &context);
        let dampen = (1.0 - WATER_DRAG * DT).max(0.0);
        assert!((body.velocity.x - 200.0 * dampen).abs() < 1e-3);
        assert!((body.velocity.y + 100.0 * dampen).abs() < 1e-3);
    }

    #[test]
    fn water_caps_terminal_fall_speed_below_the_air_limit() {
        let water = vec![Aabb::new(
            Vec2::new(-200.0, -400.0),
            Vec2::new(200.0, 400.0),
        )];
        let context = CollisionContext {
            water: &water,
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(0.0, 0.0), Vec2::splat(16.0));
        body.velocity.y = -5_000.0;
        physics_step(&mut body, 0.0, DT, &context);
        assert!((body.velocity.y + WATER_TERMINAL_FALL).abs() < 1e-3);
    }

    #[test]
    fn in_water_toggles_when_crossing_the_surface() {
        let water = vec![Aabb::new(Vec2::new(-200.0, -400.0), Vec2::new(200.0, 0.0))];
        let context = CollisionContext {
            water: &water,
            ..CollisionContext::empty()
        };
        let mut body = KinematicBody::new(Vec2::new(0.0, 40.0), Vec2::splat(16.0));
        physics_step(&mut body, GRAVITY, DT, &context);
        assert!(!body.in_water, "starts above the water line");

        while body.position.y > 0.0 {
            physics_step(&mut body, GRAVITY, DT, &context);
        }
        physics_step(&mut body, GRAVITY, DT, &context);
        assert!(body.in_water, "crossing under the surface flips the flag");

        body.position.y = 40.0;
        physics_step(&mut body, 0.0, DT, &context);
        assert!(!body.in_water, "and flips back once the center leaves");
    }
}
