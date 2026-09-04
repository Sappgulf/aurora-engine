//! Minimal deterministic finite state machines for actors, UI flow, and
//! ability logic.
//!
//! Transitions live in a flat `(state, event) -> state` table. Unknown
//! combinations are rejected rather than guessed, every `fire` returns
//! whether it applied, and [`StateMachine::trace`] exposes the recent path
//! for debugging replay tooling. States and events are plain `Copy` tokens —
//! enums fit perfectly; enter/exit side effects belong to the caller reacting
//! to the returned change.

use std::collections::HashMap;
use std::fmt::Debug;

/// A finite state machine over `Copy` states driven by `Copy` events.
///
/// ```
/// use aurora_engine::fsm::StateMachine;
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// enum Door { Shut, Open }
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// enum Input { TurnHandle, Slam }
///
/// let mut door: StateMachine<Door, Input> = StateMachine::new(Door::Shut);
/// door.allow(Door::Shut, Input::TurnHandle, Door::Open);
/// door.allow(Door::Open, Input::Slam, Door::Shut);
/// assert!(door.fire(Input::TurnHandle));
/// assert_eq!(door.current(), Door::Open);
/// // The door is open; turning the handle again has no rule.
/// assert!(!door.fire(Input::TurnHandle));
/// ```
#[derive(Debug, Clone)]
pub struct StateMachine<S, E>
where
    S: Copy + Eq + std::hash::Hash,
    E: Copy + Eq + std::hash::Hash,
{
    transitions: HashMap<(S, E), S>,
    current: S,
    trace: Vec<S>,
}

/// Number of recent states retained by the ring trace.
const TRACE_DEPTH: usize = 8;

impl<S, E> StateMachine<S, E>
where
    S: Copy + Eq + std::hash::Hash + Debug,
    E: Copy + Eq + std::hash::Hash,
{
    pub fn new(initial: S) -> Self {
        Self {
            transitions: HashMap::new(),
            current: initial,
            trace: vec![initial],
        }
    }

    /// Registers (or replaces) the target of `state` receiving `event`.
    pub fn allow(&mut self, state: S, event: E, next: S) -> &mut Self {
        self.transitions.insert((state, event), next);
        self
    }

    /// Removes a registered transition.
    pub fn forbid(&mut self, state: S, event: E) -> bool {
        self.transitions.remove(&(state, event)).is_some()
    }

    /// Where `event` leads from the current state, if legal.
    pub fn transition_for(&self, event: E) -> Option<S> {
        self.transitions.get(&(self.current, event)).copied()
    }

    /// Applies `event`. Returns `true` when a legal transition fired.
    ///
    /// Self-transitions count as fired (useful as explicit re-entry hooks).
    pub fn fire(&mut self, event: E) -> bool {
        match self.transition_for(event) {
            Some(next) => {
                let changed = next != self.current;
                self.current = next;
                if changed {
                    self.push_trace();
                }
                true
            }
            None => false,
        }
    }

    /// Forces any state directly (spawning, teleporting, test setup). Not
    /// part of the transition contract, so re-entry semantics are on you.
    pub fn force(&mut self, state: S) {
        if state != self.current {
            self.current = state;
            self.push_trace();
        }
    }

    pub fn current(&self) -> S {
        self.current
    }

    /// Most recent states, oldest first. Bounded ring ([`TRACE_DEPTH`]).
    pub fn trace(&self) -> &[S] {
        &self.trace
    }

    /// Number of registered transitions (diagnostics).
    pub fn len(&self) -> usize {
        self.transitions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.transitions.is_empty()
    }

    fn push_trace(&mut self) {
        self.trace.push(self.current);
        if self.trace.len() > TRACE_DEPTH {
            let excess = self.trace.len() - TRACE_DEPTH;
            self.trace.drain(..excess);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum Enemy {
        Patrol,
        Chase,
        Search,
        Flee,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum Cue {
        SpotPlayer,
        LosePlayer,
        FindFootprint,
        HurtBadly,
        ReachLastKnown,
    }

    fn enemy_brain() -> StateMachine<Enemy, Cue> {
        let mut fsm = StateMachine::new(Enemy::Patrol);
        fsm.allow(Enemy::Patrol, Cue::SpotPlayer, Enemy::Chase)
            .allow(Enemy::Chase, Cue::LosePlayer, Enemy::Search)
            .allow(Enemy::Search, Cue::FindFootprint, Enemy::Chase)
            .allow(Enemy::Search, Cue::ReachLastKnown, Enemy::Patrol)
            .allow(Enemy::Patrol, Cue::HurtBadly, Enemy::Flee)
            .allow(Enemy::Chase, Cue::HurtBadly, Enemy::Flee)
            .allow(Enemy::Flee, Cue::ReachLastKnown, Enemy::Patrol);
        fsm
    }

    #[test]
    fn legal_paths_drive_the_expected_walk() {
        let mut brain = enemy_brain();
        assert_eq!(brain.current(), Enemy::Patrol);
        assert!(brain.fire(Cue::SpotPlayer));
        assert_eq!(brain.current(), Enemy::Chase);
        assert!(brain.fire(Cue::LosePlayer));
        assert_eq!(brain.current(), Enemy::Search);
        assert!(brain.fire(Cue::FindFootprint));
        assert_eq!(brain.current(), Enemy::Chase);
    }

    #[test]
    fn unknown_events_are_rejected_without_state_change() {
        let mut brain = enemy_brain();
        // Searching cannot leap straight to fleeing...
        brain.force(Enemy::Search);
        assert!(!brain.fire(Cue::HurtBadly));
        assert_eq!(brain.current(), Enemy::Search);
        assert!(
            !brain.fire(Cue::SpotPlayer),
            "searching has no spot-player rule either"
        );
        assert_eq!(brain.current(), Enemy::Search);
    }

    #[test]
    fn transition_queries_do_not_mutate() {
        let brain = enemy_brain();
        assert_eq!(brain.transition_for(Cue::SpotPlayer), Some(Enemy::Chase));
        assert_eq!(brain.transition_for(Cue::LosePlayer), None);
    }

    #[test]
    fn allow_replaces_mappings_and_forbid_removes_them() {
        let mut brain = enemy_brain();
        brain.allow(Enemy::Patrol, Cue::SpotPlayer, Enemy::Flee);
        brain.fire(Cue::SpotPlayer);
        assert_eq!(brain.current(), Enemy::Flee);

        assert!(brain.forbid(Enemy::Flee, Cue::ReachLastKnown));
        assert!(!brain.fire(Cue::ReachLastKnown));
        assert_eq!(brain.len(), 6);
    }

    #[test]
    fn trace_keeps_a_bounded_recent_path() {
        let mut brain = enemy_brain();
        for cue in [
            Cue::SpotPlayer,
            Cue::LosePlayer,
            Cue::FindFootprint,
            Cue::LosePlayer,
        ] {
            brain.fire(cue);
        }
        let path = brain.trace();
        assert!(path.len() <= TRACE_DEPTH);
        assert_eq!(*path.last().expect("recent states"), Enemy::Search);
    }

    #[test]
    fn force_teleports_and_self_transitions_still_report_success() {
        let mut door = StateMachine::<i32, ()>::new(0);
        door.allow(1, (), 1);
        door.force(5);
        assert_eq!(door.current(), 5);
        door.force(1);
        assert!(door.fire(()), "explicit self-loop is a legal firing");
        assert_eq!(door.current(), 1);
    }
}
