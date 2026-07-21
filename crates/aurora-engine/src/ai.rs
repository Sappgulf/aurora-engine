//! A generic, reusable "attack nearest weak thing" opponent AI layered on
//! top of [`RtsWorld`] without touching its internals. Intended for any
//! faction-vs-faction contest (a campaign mission's enemy roster, a
//! skirmish-mode opponent, ...) — behavior is driven entirely by
//! [`AiParams`], not by any specific game's unit kinds.

use std::collections::{hash_map::Entry, HashMap, HashSet};

use glam::{IVec2, Vec2};

use crate::rts::{FactionId, NavGrid, RtsWorld, UnitId, UnitOrder};
use crate::Aabb;

/// Tunable knobs for [`SimpleAggroAi::think`]. Defaults are tuned for a
/// small-squad skirmish; missions with different pacing can override them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AiParams {
    /// Units only consider targets within this world-space radius.
    pub aggro_radius: f32,
    /// Minimum time between opportunistic re-targeting, once a target is
    /// assigned and still valid. Prevents thrashing between equally-good
    /// targets every tick.
    pub retarget_interval: f32,
    /// Health fraction (0..1) at or below which a unit disengages and
    /// retreats toward its rally point instead of attacking.
    pub retreat_health_fraction: f32,
    /// How long a retreat lasts before the unit is willing to re-engage
    /// (assuming its health has recovered above the threshold by then).
    pub retreat_duration: f32,
    /// Soft cap on simultaneous attackers per target; once reached, other
    /// attackers strongly prefer a less-covered target if one exists in
    /// range, spreading damage instead of dogpiling.
    pub max_attackers_per_target: usize,
    /// Base strength of pressure carried over across retarget events.
    /// Higher values make units spread across previously saturated targets
    /// more aggressively.
    pub pressure_weight: f32,
    /// Time in seconds for pressure to halve.
    pub pressure_half_life_secs: f32,
}

impl Default for AiParams {
    fn default() -> Self {
        Self {
            aggro_radius: 520.0,
            retarget_interval: 2.0,
            retreat_health_fraction: 0.25,
            retreat_duration: 4.0,
            max_attackers_per_target: 2,
            pressure_weight: 2_200.0,
            pressure_half_life_secs: 1.6,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AiUnitMemory {
    target: Option<UnitId>,
    target_since: f32,
    retreat_until: f32,
    rally_point: Vec2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PathCacheKey {
    from: IVec2,
    to: IVec2,
    nav_version: u64,
}

#[derive(Debug, Clone)]
struct CachedPath {
    waypoints: Vec<Vec2>,
    last_used_at: f64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PathingStats {
    pub segment_block_checks: u64,
    pub path_cache_hits: u64,
    pub path_cache_misses: u64,
    pub path_cache_evictions: u64,
    pub paths_planned: u64,
    pub path_segments_returned: u64,
}

#[derive(Debug, Clone, Copy)]
struct TargetPressure {
    value: f32,
    updated_at: f64,
}

/// Per-attacker-faction memory (assigned target, retreat timers, rally
/// point) driving a simple aggro-and-retreat behavior each tick.
#[derive(Debug, Clone, Default)]
pub struct SimpleAggroAi {
    memory: HashMap<UnitId, AiUnitMemory>,
    path_cache: HashMap<PathCacheKey, CachedPath>,
    target_pressure: HashMap<UnitId, TargetPressure>,
    cached_nav_version: Option<u64>,
    pathing_stats: PathingStats,
}

impl SimpleAggroAi {
    const PATH_CACHE_MAX_ENTRIES: usize = 2048;
    const PATH_CACHE_TTL_SECONDS: f64 = 6.0;

    pub fn new() -> Self {
        Self::default()
    }

    /// Assigns orders to every alive `attackers`-faction unit against alive
    /// `targets`-faction units. Call once per AI think-tick (the caller
    /// decides cadence, e.g. every 0.5s) with a monotonically increasing
    /// `elapsed` clock. Pass a `nav` grid to route the approach around
    /// obstacles registered with [`mark_obstacles`]; pass `None` to always
    /// approach in a straight line.
    pub fn think(
        &mut self,
        world: &mut RtsWorld,
        attackers: FactionId,
        targets: FactionId,
        elapsed: f32,
        params: &AiParams,
        nav: Option<&NavGrid>,
    ) {
        self.sync_path_cache(nav);
        self.evict_stale_path_cache_entries(elapsed);
        self.evict_excess_path_cache_entries();
        let candidates: Vec<(UnitId, Vec2, f32)> = world
            .units()
            .iter()
            .filter(|unit| unit.faction == targets && unit.alive())
            .map(|unit| {
                let health_fraction = (unit.health / unit.max_health.max(1.0)).clamp(0.0, 1.0);
                (unit.id, unit.position, health_fraction)
            })
            .collect();

        let attacker_ids: Vec<UnitId> = world
            .units()
            .iter()
            .filter(|unit| unit.faction == attackers && unit.alive())
            .map(|unit| unit.id)
            .collect();

        // Drop stale memory so a long-running mission doesn't accumulate
        // entries for units that died or never belonged to this faction.
        let attacker_set: std::collections::HashSet<UnitId> =
            attacker_ids.iter().copied().collect();
        self.memory.retain(|id, _| attacker_set.contains(id));

        let candidate_ids: HashSet<UnitId> = candidates.iter().map(|(id, _, _)| *id).collect();
        self.prune_and_decay_pressure(elapsed, params, &candidate_ids);
        let mut assigned_counts: HashMap<UnitId, usize> = HashMap::new();
        for memory in self.memory.values() {
            if let Some(target) = memory.target {
                // Do not let a dead/out-of-scope target consume the soft cap
                // for the rest of the match.
                if candidate_ids.contains(&target) {
                    *assigned_counts.entry(target).or_insert(0) += 1;
                }
            }
        }

        for id in attacker_ids {
            let Some((position, health_fraction)) = world.unit(id).map(|unit| {
                (
                    unit.position,
                    (unit.health / unit.max_health.max(1.0)).clamp(0.0, 1.0),
                )
            }) else {
                continue;
            };

            let rally_point = self
                .memory
                .get(&id)
                .map(|memory| memory.rally_point)
                .unwrap_or(position);
            // Work on a local copy so target scoring can borrow pressure state
            // without overlapping a mutable entry borrow. The updated memory
            // is written back on every exit path below.
            let mut memory = self.memory.get(&id).copied().unwrap_or(AiUnitMemory {
                target: None,
                target_since: elapsed,
                retreat_until: f32::MIN,
                rally_point,
            });

            if health_fraction <= params.retreat_health_fraction && elapsed >= memory.retreat_until
            {
                memory.retreat_until = elapsed + params.retreat_duration;
                if let Some(previous_target) = memory.target.take() {
                    decrement_assignment(&mut assigned_counts, previous_target);
                }
            }
            if elapsed < memory.retreat_until {
                if let Some(unit) = world.unit_mut(id) {
                    unit.order = UnitOrder::Move(memory.rally_point);
                }
                self.memory.insert(id, memory);
                continue;
            }

            let current_valid = memory.target.and_then(|target_id| {
                candidates
                    .iter()
                    .find(|(id, _, _)| *id == target_id)
                    .filter(|(_, target_position, _)| {
                        target_position.distance(position) <= params.aggro_radius
                    })
                    .copied()
            });
            let should_retarget = current_valid.is_none()
                || elapsed - memory.target_since >= params.retarget_interval;

            // The counts above describe the assignments entering this tick.
            // Remove this unit's previous assignment before choosing a new
            // target, then add exactly one assignment below.
            if should_retarget {
                if let Some(previous_target) = memory.target.take() {
                    decrement_assignment(&mut assigned_counts, previous_target);
                }
            }

            let chosen = if should_retarget {
                candidates
                    .iter()
                    .filter(|(_, target_position, _)| {
                        target_position.distance(position) <= params.aggro_radius
                    })
                    .copied()
                    .min_by(|a, b| {
                        score(
                            position,
                            *a,
                            &assigned_counts,
                            self.target_pressure
                                .get(&a.0)
                                .map_or(0.0, |pressure| pressure.value),
                            params,
                        )
                        .total_cmp(&score(
                            position,
                            *b,
                            &assigned_counts,
                            self.target_pressure
                                .get(&b.0)
                                .map_or(0.0, |pressure| pressure.value),
                            params,
                        ))
                    })
            } else {
                current_valid
            };

            let Some((target_id, target_position, _)) = chosen else {
                memory.target = None;
                if let Some(unit) = world.unit_mut(id) {
                    unit.order = UnitOrder::Idle;
                }
                self.memory.insert(id, memory);
                continue;
            };

            if memory.target != Some(target_id) {
                memory.target = Some(target_id);
                memory.target_since = elapsed;
            }
            if should_retarget {
                *assigned_counts.entry(target_id).or_insert(0) += 1;
                self.note_target_pressure(target_id, elapsed);
            }

            let order = approach_order(
                nav,
                position,
                target_position,
                target_id,
                elapsed,
                &mut self.path_cache,
                &mut self.pathing_stats,
            );
            if let Some(unit) = world.unit_mut(id) {
                unit.order = order;
            }
            self.memory.insert(id, memory);
        }
    }
}

impl SimpleAggroAi {
    pub fn pathing_stats(&self) -> PathingStats {
        self.pathing_stats
    }

    pub fn clear_pathing_stats(&mut self) {
        self.pathing_stats = PathingStats::default();
    }

    fn sync_path_cache(&mut self, nav: Option<&NavGrid>) {
        let current_nav_version = nav.map(|nav| nav.version());
        if self.cached_nav_version != current_nav_version {
            self.path_cache.clear();
            self.cached_nav_version = current_nav_version;
        }
    }

    fn evict_stale_path_cache_entries(&mut self, elapsed: f32) {
        if !elapsed.is_finite() {
            let evicted = u64::try_from(self.path_cache.len()).unwrap_or(u64::MAX);
            self.pathing_stats.path_cache_evictions = self
                .pathing_stats
                .path_cache_evictions
                .saturating_add(evicted);
            self.path_cache.clear();
            return;
        }
        let cutoff = f64::from(elapsed) - Self::PATH_CACHE_TTL_SECONDS;
        let before = self.path_cache.len();
        self.path_cache
            .retain(|_, path| path.last_used_at >= cutoff);
        let removed = before.saturating_sub(self.path_cache.len());
        self.pathing_stats.path_cache_evictions = self
            .pathing_stats
            .path_cache_evictions
            .saturating_add(u64::try_from(removed).unwrap_or(u64::MAX));
    }

    fn evict_excess_path_cache_entries(&mut self) {
        let over_capacity = self
            .path_cache
            .len()
            .saturating_sub(Self::PATH_CACHE_MAX_ENTRIES);
        if over_capacity == 0 {
            return;
        }
        let mut keys: Vec<(f64, PathCacheKey)> = self
            .path_cache
            .iter()
            .map(|(key, path)| (path.last_used_at, *key))
            .collect();
        keys.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));

        for (_, key) in keys.into_iter().take(over_capacity) {
            if self.path_cache.remove(&key).is_some() {
                self.pathing_stats.path_cache_evictions =
                    self.pathing_stats.path_cache_evictions.saturating_add(1);
            }
        }
    }

    fn prune_and_decay_pressure(
        &mut self,
        elapsed: f32,
        params: &AiParams,
        targets: &HashSet<UnitId>,
    ) {
        if !elapsed.is_finite() {
            return;
        }
        let now = f64::from(elapsed);
        self.target_pressure
            .retain(|target_id, _| targets.contains(target_id));

        if params.pressure_half_life_secs <= f32::EPSILON {
            return;
        }
        let decay_base = 0.5_f32;
        let half_life = params.pressure_half_life_secs;
        for pressure in self.target_pressure.values_mut() {
            let age = (now - pressure.updated_at) as f32;
            if age <= 0.0 {
                continue;
            }
            let decay = decay_base.powf(age / half_life);
            pressure.value *= decay;
            pressure.updated_at = now;
        }
        self.target_pressure
            .retain(|_, pressure| pressure.value > 0.000_1);
    }

    fn note_target_pressure(&mut self, target_id: UnitId, elapsed: f32) {
        if !elapsed.is_finite() {
            return;
        }
        let now = f64::from(elapsed);
        let entry = self
            .target_pressure
            .entry(target_id)
            .or_insert(TargetPressure {
                value: 0.0,
                updated_at: now,
            });
        entry.value += 1.0;
        entry.updated_at = now;
    }
}

fn decrement_assignment(assigned: &mut HashMap<UnitId, usize>, target: UnitId) {
    let Some(count) = assigned.get_mut(&target) else {
        return;
    };
    if *count <= 1 {
        assigned.remove(&target);
    } else {
        *count -= 1;
    }
}

fn score(
    from: Vec2,
    candidate: (UnitId, Vec2, f32),
    assigned: &HashMap<UnitId, usize>,
    pressure: f32,
    params: &AiParams,
) -> f32 {
    let (id, position, health_fraction) = candidate;
    let distance = from.distance(position);
    let attacker_count = assigned.get(&id).copied().unwrap_or(0);
    let coverage_penalty = if attacker_count >= params.max_attackers_per_target {
        100_000.0
    } else {
        attacker_count as f32 * 220.0
    };
    let pressure_penalty = pressure * params.pressure_weight;
    // Lower is better: prefer close, low-health (near-kill), lightly-covered
    // targets.
    distance + health_fraction * 260.0 + coverage_penalty + pressure_penalty
}

fn approach_order(
    nav: Option<&NavGrid>,
    from: Vec2,
    target_position: Vec2,
    target_id: UnitId,
    elapsed: f32,
    path_cache: &mut HashMap<PathCacheKey, CachedPath>,
    path_stats: &mut PathingStats,
) -> UnitOrder {
    if let Some(nav) = nav {
        path_stats.segment_block_checks = path_stats.segment_block_checks.saturating_add(1);
        if nav.segment_blocked(from, target_position) {
            let start = nav.snap_to_cell_center(from);
            let goal = nav.snap_to_cell_center(target_position);
            let key = PathCacheKey {
                from: nav.world_to_cell(start),
                to: nav.world_to_cell(goal),
                nav_version: nav.version(),
            };
            let waypoint = match path_cache.entry(key) {
                Entry::Occupied(mut entry) => {
                    path_stats.path_cache_hits = path_stats.path_cache_hits.saturating_add(1);
                    entry.get_mut().last_used_at = f64::from(elapsed);
                    next_waypoint(from, &entry.get().waypoints)
                }
                Entry::Vacant(entry) => {
                    path_stats.path_cache_misses = path_stats.path_cache_misses.saturating_add(1);
                    let waypoints = nav.find_path(start, goal);
                    path_stats.paths_planned = path_stats.paths_planned.saturating_add(1);
                    let cached = entry.insert(CachedPath {
                        waypoints,
                        last_used_at: f64::from(elapsed),
                    });
                    next_waypoint(from, &cached.waypoints)
                }
            };
            if let Some(waypoint) = waypoint {
                path_stats.path_segments_returned =
                    path_stats.path_segments_returned.saturating_add(1);
                return UnitOrder::Move(waypoint);
            }
        }
    }
    UnitOrder::Attack(target_id)
}

fn next_waypoint(from: Vec2, path: &[Vec2]) -> Option<Vec2> {
    let reach_threshold = 1.0_f32;
    path.iter()
        .copied()
        .find(|waypoint| from.distance(*waypoint) > reach_threshold)
}

/// Marks every `NavGrid` cell overlapping any of `obstacles` as blocked.
/// Convenience for building a grid from a mission/skirmish map's static
/// structure footprints.
pub fn mark_obstacles(grid: &mut NavGrid, obstacles: &[Aabb]) {
    for obstacle in obstacles {
        let min_cell = grid.world_to_cell(obstacle.min);
        let max_cell = grid.world_to_cell(obstacle.max);
        grid.set_blocked_rect(min_cell, max_cell, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATTACKERS: FactionId = FactionId(2);
    const TARGETS: FactionId = FactionId(1);

    #[test]
    fn target_selection_prefers_lower_health_targets() {
        let mut world = RtsWorld::default();
        let attacker = world.spawn(ATTACKERS, Vec2::ZERO);
        let weak = world.spawn(TARGETS, Vec2::new(100.0, 0.0));
        let strong = world.spawn(TARGETS, Vec2::new(100.0, 0.0));
        world.unit_mut(weak).unwrap().health = 10.0;
        world.unit_mut(strong).unwrap().health = 100.0;

        let mut ai = SimpleAggroAi::new();
        ai.think(
            &mut world,
            ATTACKERS,
            TARGETS,
            0.0,
            &AiParams::default(),
            None,
        );

        assert_eq!(world.unit(attacker).unwrap().order, UnitOrder::Attack(weak));
    }

    #[test]
    fn low_health_units_retreat_then_recover() {
        let mut world = RtsWorld::default();
        let unit_id = world.spawn(ATTACKERS, Vec2::ZERO);
        world.spawn(TARGETS, Vec2::new(50.0, 0.0));
        {
            let unit = world.unit_mut(unit_id).unwrap();
            unit.health = 10.0;
            unit.max_health = 100.0;
        }

        let params = AiParams {
            retreat_health_fraction: 0.25,
            retreat_duration: 4.0,
            ..Default::default()
        };
        let mut ai = SimpleAggroAi::new();

        ai.think(&mut world, ATTACKERS, TARGETS, 0.0, &params, None);
        assert!(matches!(
            world.unit(unit_id).unwrap().order,
            UnitOrder::Move(_)
        ));

        // Retreat window (0..4) has elapsed but health is still low, so it
        // should re-trigger another retreat window instead of re-engaging.
        ai.think(&mut world, ATTACKERS, TARGETS, 5.0, &params, None);
        assert!(matches!(
            world.unit(unit_id).unwrap().order,
            UnitOrder::Move(_)
        ));

        world.unit_mut(unit_id).unwrap().health = 90.0;
        // Now past the second retreat window (5..9) with health recovered.
        ai.think(&mut world, ATTACKERS, TARGETS, 9.1, &params, None);
        assert!(matches!(
            world.unit(unit_id).unwrap().order,
            UnitOrder::Attack(_)
        ));
    }

    #[test]
    fn attackers_spread_across_available_targets() {
        let mut world = RtsWorld::default();
        let a1 = world.spawn(ATTACKERS, Vec2::ZERO);
        let a2 = world.spawn(ATTACKERS, Vec2::new(1.0, 0.0));
        let t1 = world.spawn(TARGETS, Vec2::new(100.0, 0.0));
        let t2 = world.spawn(TARGETS, Vec2::new(100.0, 1.0));

        let mut ai = SimpleAggroAi::new();
        ai.think(
            &mut world,
            ATTACKERS,
            TARGETS,
            0.0,
            &AiParams::default(),
            None,
        );

        let order_a1 = world.unit(a1).unwrap().order;
        let order_a2 = world.unit(a2).unwrap().order;
        assert_ne!(order_a1, order_a2);
        assert!(matches!(order_a1, UnitOrder::Attack(id) if id == t1 || id == t2));
        assert!(matches!(order_a2, UnitOrder::Attack(id) if id == t1 || id == t2));
    }

    #[test]
    fn repeated_retargeting_does_not_inflate_assignments() {
        let mut world = RtsWorld::default();
        let a1 = world.spawn(ATTACKERS, Vec2::ZERO);
        let a2 = world.spawn(ATTACKERS, Vec2::new(1.0, 0.0));
        let t1 = world.spawn(TARGETS, Vec2::new(100.0, 0.0));
        let t2 = world.spawn(TARGETS, Vec2::new(120.0, 0.0));

        let mut ai = SimpleAggroAi::new();
        for elapsed in [0.0, 2.1, 4.2, 6.3] {
            ai.think(
                &mut world,
                ATTACKERS,
                TARGETS,
                elapsed,
                &AiParams::default(),
                None,
            );
            let order_a1 = world.unit(a1).unwrap().order;
            let order_a2 = world.unit(a2).unwrap().order;
            assert!(matches!(order_a1, UnitOrder::Attack(id) if id == t1 || id == t2));
            assert!(matches!(order_a2, UnitOrder::Attack(id) if id == t1 || id == t2));
            assert_ne!(
                order_a1, order_a2,
                "retarget tick {elapsed} should spread fire"
            );
        }
    }

    #[test]
    fn approach_routes_around_blocked_segment() {
        let mut grid = NavGrid::new(5, 3, Vec2::ZERO, 10.0);
        let mut path_cache = std::collections::HashMap::new();
        let mut pathing_stats = PathingStats::default();
        mark_obstacles(
            &mut grid,
            &[Aabb::from_center_size(
                Vec2::new(25.0, 15.0),
                Vec2::splat(8.0),
            )],
        );
        let order = approach_order(
            Some(&grid),
            Vec2::new(5.0, 15.0),
            Vec2::new(45.0, 15.0),
            UnitId(0),
            0.0,
            &mut path_cache,
            &mut pathing_stats,
        );
        assert!(matches!(order, UnitOrder::Move(_)));
        assert_eq!(pathing_stats.path_cache_misses, 1);
        assert_eq!(pathing_stats.path_segments_returned, 1);

        let clear_order = approach_order(
            Some(&grid),
            Vec2::new(5.0, 5.0),
            Vec2::new(45.0, 5.0),
            UnitId(0),
            0.0,
            &mut path_cache,
            &mut pathing_stats,
        );
        assert!(matches!(clear_order, UnitOrder::Attack(_)));
    }

    #[test]
    fn cached_paths_report_hits_then_expire_after_the_ttl() {
        let mut world = RtsWorld::default();
        let attacker = world.spawn(ATTACKERS, Vec2::new(5.0, 15.0));
        world.spawn(TARGETS, Vec2::new(65.0, 15.0));
        let mut grid = NavGrid::new(7, 3, Vec2::ZERO, 10.0);
        grid.set_blocked(IVec2::new(3, 1), true);

        let mut ai = SimpleAggroAi::new();
        ai.think(
            &mut world,
            ATTACKERS,
            TARGETS,
            0.0,
            &AiParams::default(),
            Some(&grid),
        );
        ai.clear_pathing_stats();
        ai.think(
            &mut world,
            ATTACKERS,
            TARGETS,
            3.0,
            &AiParams::default(),
            Some(&grid),
        );
        assert_eq!(ai.pathing_stats().path_cache_hits, 1);
        assert_eq!(ai.pathing_stats().path_cache_misses, 0);
        assert!(matches!(
            world.unit(attacker).unwrap().order,
            UnitOrder::Move(_)
        ));

        ai.clear_pathing_stats();
        ai.think(
            &mut world,
            ATTACKERS,
            TARGETS,
            10.0,
            &AiParams::default(),
            Some(&grid),
        );
        assert_eq!(ai.pathing_stats().path_cache_misses, 1);
        assert_eq!(ai.pathing_stats().path_cache_hits, 0);
    }

    #[test]
    fn cached_path_recomputed_after_nav_update() {
        let mut grid = NavGrid::new(7, 3, Vec2::ZERO, 10.0);
        let mut path_cache = std::collections::HashMap::new();
        let mut pathing_stats = PathingStats::default();
        let from = Vec2::new(5.0, 15.0);
        let to = Vec2::new(65.0, 15.0);

        grid.set_blocked(IVec2::new(3, 1), true);
        let first = approach_order(
            Some(&grid),
            from,
            to,
            UnitId(1),
            0.0,
            &mut path_cache,
            &mut pathing_stats,
        );
        assert!(matches!(first, UnitOrder::Move(_)));

        // Tighten the choke point to a solid wall and verify cached path
        // entries from the previous nav version are not reused.
        grid.set_blocked(IVec2::new(3, 0), true);
        grid.set_blocked(IVec2::new(3, 2), true);
        let second = approach_order(
            Some(&grid),
            from,
            to,
            UnitId(1),
            1.0,
            &mut path_cache,
            &mut pathing_stats,
        );
        assert!(matches!(second, UnitOrder::Attack(_)));
    }

    #[test]
    fn cached_path_entries_reset_when_nav_version_changes() {
        let mut world = RtsWorld::default();
        let attacker = world.spawn(ATTACKERS, Vec2::new(5.0, 15.0));
        world.spawn(TARGETS, Vec2::new(65.0, 15.0));

        let mut grid = NavGrid::new(7, 3, Vec2::ZERO, 10.0);
        grid.set_blocked(IVec2::new(3, 1), true);

        let mut ai = SimpleAggroAi::new();
        ai.think(
            &mut world,
            ATTACKERS,
            TARGETS,
            0.0,
            &AiParams::default(),
            Some(&grid),
        );
        assert_eq!(ai.path_cache.len(), 1);
        assert!(matches!(
            world.unit(attacker).unwrap().order,
            UnitOrder::Move(_)
        ));

        grid.set_blocked(IVec2::new(3, 0), true);
        grid.set_blocked(IVec2::new(3, 2), true);
        ai.think(
            &mut world,
            ATTACKERS,
            TARGETS,
            3.1,
            &AiParams::default(),
            Some(&grid),
        );
        assert_eq!(ai.path_cache.len(), 1);
        assert!(matches!(
            world.unit(attacker).unwrap().order,
            UnitOrder::Attack(_)
        ));
    }

    #[test]
    fn path_cache_evicts_oldest_entries_when_over_capacity() {
        let mut world = RtsWorld::default();
        let mut ai = SimpleAggroAi::new();
        let cap = SimpleAggroAi::PATH_CACHE_MAX_ENTRIES;
        for idx in 0..(cap + 7) {
            ai.path_cache.insert(
                PathCacheKey {
                    from: IVec2::new(idx as i32, 0),
                    to: IVec2::new(idx as i32 + 1, 0),
                    nav_version: 0,
                },
                CachedPath {
                    waypoints: Vec::new(),
                    last_used_at: idx as f64,
                },
            );
        }

        ai.think(
            &mut world,
            ATTACKERS,
            TARGETS,
            1.0,
            &AiParams::default(),
            None,
        );

        assert_eq!(ai.path_cache.len(), cap);
        assert!(!ai.path_cache.contains_key(&PathCacheKey {
            from: IVec2::new(0, 0),
            to: IVec2::new(1, 0),
            nav_version: 0,
        }));
        assert!(ai.path_cache.contains_key(&PathCacheKey {
            from: IVec2::new(cap as i32 + 6, 0),
            to: IVec2::new(cap as i32 + 7, 0),
            nav_version: 0,
        }));
        assert_eq!(ai.pathing_stats.path_cache_evictions, 7);
    }

    #[test]
    fn target_pressure_decays_so_a_previously_skipped_target_can_return() {
        let mut world = RtsWorld::default();
        let attacker = world.spawn(ATTACKERS, Vec2::ZERO);
        let first = world.spawn(TARGETS, Vec2::new(100.0, 0.0));
        let second = world.spawn(TARGETS, Vec2::new(100.0, 1.0));
        let params = AiParams {
            retarget_interval: 1.0,
            pressure_half_life_secs: 2.0,
            ..Default::default()
        };
        let mut ai = SimpleAggroAi::new();

        ai.think(&mut world, ATTACKERS, TARGETS, 0.0, &params, None);
        assert_eq!(
            world.unit(attacker).unwrap().order,
            UnitOrder::Attack(first)
        );

        ai.think(&mut world, ATTACKERS, TARGETS, 1.1, &params, None);
        assert_eq!(
            world.unit(attacker).unwrap().order,
            UnitOrder::Attack(second)
        );

        // The first target's pressure has had time to decay below the second
        // target's newer pressure, so deterministic scoring returns to it.
        ai.think(&mut world, ATTACKERS, TARGETS, 3.1, &params, None);
        assert_eq!(
            world.unit(attacker).unwrap().order,
            UnitOrder::Attack(first)
        );
    }
}
