//! A generic, reusable "attack nearest weak thing" opponent AI layered on
//! top of [`RtsWorld`] without touching its internals. Intended for any
//! faction-vs-faction contest (a campaign mission's enemy roster, a
//! skirmish-mode opponent, ...) — behavior is driven entirely by
//! [`AiParams`], not by any specific game's unit kinds.

use std::collections::{HashMap, HashSet};

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
}

impl Default for AiParams {
    fn default() -> Self {
        Self {
            aggro_radius: 520.0,
            retarget_interval: 2.0,
            retreat_health_fraction: 0.25,
            retreat_duration: 4.0,
            max_attackers_per_target: 2,
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
}

/// Per-attacker-faction memory (assigned target, retreat timers, rally
/// point) driving a simple aggro-and-retreat behavior each tick.
#[derive(Debug, Clone, Default)]
pub struct SimpleAggroAi {
    memory: HashMap<UnitId, AiUnitMemory>,
    path_cache: HashMap<PathCacheKey, Option<Vec2>>,
}

impl SimpleAggroAi {
    const PATH_CACHE_MAX_ENTRIES: usize = 2048;

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
        if self.path_cache.len() > Self::PATH_CACHE_MAX_ENTRIES {
            self.path_cache.clear();
        }
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
            let memory = self.memory.entry(id).or_insert(AiUnitMemory {
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
                        score(position, *a, &assigned_counts, params).total_cmp(&score(
                            position,
                            *b,
                            &assigned_counts,
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
                continue;
            };

            if memory.target != Some(target_id) {
                memory.target = Some(target_id);
                memory.target_since = elapsed;
            }
            if should_retarget {
                *assigned_counts.entry(target_id).or_insert(0) += 1;
            }

            let order = approach_order(
                nav,
                position,
                target_position,
                target_id,
                &mut self.path_cache,
            );
            if let Some(unit) = world.unit_mut(id) {
                unit.order = order;
            }
        }
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
    // Lower is better: prefer close, low-health (near-kill), lightly-covered
    // targets.
    distance + health_fraction * 260.0 + coverage_penalty
}

fn approach_order(
    nav: Option<&NavGrid>,
    from: Vec2,
    target_position: Vec2,
    target_id: UnitId,
    path_cache: &mut HashMap<PathCacheKey, Option<Vec2>>,
) -> UnitOrder {
    if let Some(nav) = nav {
        if nav.segment_blocked(from, target_position) {
            let key = PathCacheKey {
                from: nav.world_to_cell(from),
                to: nav.world_to_cell(target_position),
            };
            let maybe_waypoint = path_cache
                .entry(key)
                .or_insert_with(|| nav.find_path(from, target_position).first().copied());
            if let Some(waypoint) = maybe_waypoint {
                return UnitOrder::Move(*waypoint);
            }
        }
    }
    UnitOrder::Attack(target_id)
}

/// Marks every `NavGrid` cell overlapping any of `obstacles` as blocked.
/// Convenience for building a grid from a mission/skirmish map's static
/// structure footprints.
pub fn mark_obstacles(grid: &mut NavGrid, obstacles: &[Aabb]) {
    for obstacle in obstacles {
        let min_cell = grid.world_to_cell(obstacle.min);
        let max_cell = grid.world_to_cell(obstacle.max);
        for y in min_cell.y..=max_cell.y {
            for x in min_cell.x..=max_cell.x {
                grid.set_blocked(IVec2::new(x, y), true);
            }
        }
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
            &mut path_cache,
        );
        assert!(matches!(order, UnitOrder::Move(_)));

        let clear_order = approach_order(
            Some(&grid),
            Vec2::new(5.0, 5.0),
            Vec2::new(45.0, 5.0),
            UnitId(0),
            &mut path_cache,
        );
        assert!(matches!(clear_order, UnitOrder::Attack(_)));
    }
}
