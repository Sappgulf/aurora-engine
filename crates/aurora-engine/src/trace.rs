//! Deterministic semantic command traces for headless scenarios and replays.
//!
//! Aurora owns tick scheduling and the portable trace envelope. Games retain
//! ownership of action names, payload schemas, command validation, and the
//! state included in their deterministic hash.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const TRACE_FORMAT_VERSION: u32 = 1;
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StateHash(pub u64);

impl fmt::Display for StateHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:016x}", self.0)
    }
}

/// A deliberately specified byte hasher rather than Rust's implementation-
/// dependent default hasher. Games can feed integer/fixed-point state directly
/// when serialized snapshots are not appropriate.
#[derive(Debug, Clone)]
pub struct StableStateHasher {
    value: u64,
}

impl Default for StableStateHasher {
    fn default() -> Self {
        Self {
            value: FNV_OFFSET_BASIS,
        }
    }
}

impl StableStateHasher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.value ^= u64::from(*byte);
            self.value = self.value.wrapping_mul(FNV_PRIME);
        }
    }

    pub fn write_u64(&mut self, value: u64) {
        self.write_bytes(&value.to_le_bytes());
    }

    pub fn write_i64(&mut self, value: i64) {
        self.write_bytes(&value.to_le_bytes());
    }

    pub fn write_bool(&mut self, value: bool) {
        self.write_bytes(&[u8::from(value)]);
    }

    pub fn finish(&self) -> StateHash {
        StateHash(self.value)
    }
}

/// Convenience hashing for snapshots built only from deterministically
/// ordered data. Games containing `HashMap` state should instead feed sorted
/// entries into [`StableStateHasher`] explicitly.
pub fn hash_serializable<T: Serialize>(state: &T) -> Result<StateHash, TraceError> {
    let bytes = serde_json::to_vec(state).map_err(TraceError::Encode)?;
    let mut hasher = StableStateHasher::new();
    hasher.write_bytes(&bytes);
    Ok(hasher.finish())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticCommand {
    pub tick: u64,
    pub action: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub payload: Value,
}

impl SemanticCommand {
    pub fn new(tick: u64, action: impl Into<String>) -> Self {
        Self {
            tick,
            action: action.into(),
            payload: Value::Null,
        }
    }

    pub fn with_payload<T: Serialize>(mut self, payload: &T) -> Result<Self, TraceError> {
        self.payload = serde_json::to_value(payload).map_err(TraceError::Encode)?;
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuroraTrace {
    pub format_version: u32,
    pub scenario_id: String,
    pub seed: u64,
    pub fixed_tick_hz: u32,
    pub end_tick: u64,
    pub commands: Vec<SemanticCommand>,
}

impl AuroraTrace {
    pub fn new(
        scenario_id: impl Into<String>,
        seed: u64,
        fixed_tick_hz: u32,
        end_tick: u64,
    ) -> Self {
        Self {
            format_version: TRACE_FORMAT_VERSION,
            scenario_id: scenario_id.into(),
            seed,
            fixed_tick_hz,
            end_tick,
            commands: Vec::new(),
        }
    }

    pub fn push(&mut self, command: SemanticCommand) {
        self.commands.push(command);
    }

    pub fn validate(&self) -> Result<(), TraceError> {
        if self.format_version != TRACE_FORMAT_VERSION {
            return Err(TraceError::InvalidTrace(format!(
                "unsupported trace version {}",
                self.format_version
            )));
        }
        if self.scenario_id.trim().is_empty() {
            return Err(TraceError::InvalidTrace("scenario id is empty".to_owned()));
        }
        if self.fixed_tick_hz == 0 {
            return Err(TraceError::InvalidTrace(
                "fixed tick rate must be greater than zero".to_owned(),
            ));
        }
        let mut previous_tick = 0;
        for (index, command) in self.commands.iter().enumerate() {
            if command.action.trim().is_empty() {
                return Err(TraceError::InvalidTrace(format!(
                    "command {index} has an empty action"
                )));
            }
            if command.tick >= self.end_tick {
                return Err(TraceError::InvalidTrace(format!(
                    "command {} is outside the trace end tick {}",
                    command.tick, self.end_tick
                )));
            }
            if index > 0 && command.tick < previous_tick {
                return Err(TraceError::InvalidTrace(
                    "commands must be ordered by tick".to_owned(),
                ));
            }
            previous_tick = command.tick;
        }
        Ok(())
    }

    pub fn to_json_pretty(&self) -> Result<String, TraceError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(TraceError::Encode)
    }

    pub fn from_json(contents: &str) -> Result<Self, TraceError> {
        let trace: Self = serde_json::from_str(contents).map_err(TraceError::Decode)?;
        trace.validate()?;
        Ok(trace)
    }
}

/// Optional tick-level checkpoint for deterministic replay validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceCheckpoint {
    /// Tick index after fixed-step simulation has advanced this many times.
    pub tick: u64,
    /// Expected stable state hash at this tick.
    pub expected_hash: StateHash,
    /// Optional marker shown in checkpoint mismatch diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl TraceCheckpoint {
    pub fn new(tick: u64, expected_hash: StateHash, label: Option<&'static str>) -> Self {
        Self {
            tick,
            expected_hash,
            label: label.map(std::string::ToString::to_string),
        }
    }
}

/// A renderer-free simulation capable of consuming game-owned semantic
/// commands. Implementations should construct a fresh instance with the
/// trace's seed before each run.
pub trait DeterministicSimulation {
    fn apply_command(&mut self, command: &SemanticCommand) -> Result<(), String>;
    fn fixed_step(&mut self);
    fn state_hash(&self) -> StateHash;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceRunReport {
    pub scenario_id: String,
    pub seed: u64,
    pub ticks_executed: u64,
    pub commands_applied: usize,
    pub final_state_hash: StateHash,
    pub checkpoints_checked: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceStepRecord {
    /// Fixed tick whose completed state this record describes.
    pub tick: u64,
    /// Alias of [`Self::tick`] kept for explicitness in replay tooling.
    pub fixed_tick: u64,
    /// Number of trace commands consumed *before* this state was produced.
    ///
    /// A command scheduled at tick `t` is applied immediately before the
    /// step that produces state `t + 1`, so it first appears in the record
    /// labeled `t + 1`. This matches the command-to-step adjacency that
    /// every checked-in gameplay trace replays against.
    pub commands_applied: usize,
    pub state_hash: StateHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceRunJournal {
    pub records: Vec<TraceStepRecord>,
}

impl TraceRunJournal {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    pub fn push(&mut self, record: TraceStepRecord) {
        self.records.push(record);
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn final_hash(&self) -> Option<StateHash> {
        self.records.last().map(|record| record.state_hash.clone())
    }
}

impl Default for TraceRunJournal {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_trace<S: DeterministicSimulation>(
    simulation: &mut S,
    trace: &AuroraTrace,
) -> Result<TraceRunReport, TraceError> {
    run_trace_with_checkpoints_and_journal(simulation, trace, &[], None)
}

/// Run a deterministic simulation while optionally checking fixed tick
/// checkpoints. A checkpoint is validated **after** the fixed-step simulation
/// for the requested tick has completed.
pub fn run_trace_with_checkpoints<S: DeterministicSimulation>(
    simulation: &mut S,
    trace: &AuroraTrace,
    checkpoints: &[TraceCheckpoint],
) -> Result<TraceRunReport, TraceError> {
    run_trace_with_checkpoints_and_journal(simulation, trace, checkpoints, None)
}

/// Run a deterministic simulation while optionally collecting per-tick journal
/// entries for replay diffing.
pub fn run_trace_with_checkpoints_and_journal<S: DeterministicSimulation>(
    simulation: &mut S,
    trace: &AuroraTrace,
    checkpoints: &[TraceCheckpoint],
    mut journal: Option<&mut TraceRunJournal>,
) -> Result<TraceRunReport, TraceError> {
    trace.validate()?;
    let mut checkpoints = checkpoints.to_vec();
    checkpoints.sort_by_key(|checkpoint| checkpoint.tick);
    if checkpoints
        .iter()
        .any(|checkpoint| checkpoint.tick > trace.end_tick)
    {
        return Err(TraceError::InvalidTrace(
            "trace checkpoint tick exceeds end tick".to_owned(),
        ));
    }

    for window in checkpoints.windows(2) {
        if window[0].tick > window[1].tick {
            return Err(TraceError::InvalidTrace(
                "trace checkpoints must be sorted by tick".to_owned(),
            ));
        }
    }

    let mut cursor = 0;
    let mut next_checkpoint = 0;
    let mut checkpoints_checked = 0;
    for tick in 0..trace.end_tick {
        while cursor < trace.commands.len() && trace.commands[cursor].tick == tick {
            let command = &trace.commands[cursor];
            simulation
                .apply_command(command)
                .map_err(|message| TraceError::Command {
                    tick,
                    action: command.action.clone(),
                    message,
                })?;
            cursor += 1;
        }
        simulation.fixed_step();

        let current_tick = tick + 1;
        let hash = simulation.state_hash();
        if let Some(journal) = journal.as_mut() {
            journal.push(TraceStepRecord {
                tick: current_tick,
                fixed_tick: current_tick,
                commands_applied: cursor,
                state_hash: hash.clone(),
            });
        }
        while next_checkpoint < checkpoints.len()
            && checkpoints[next_checkpoint].tick == current_tick
        {
            let checkpoint = &checkpoints[next_checkpoint];
            if hash != checkpoint.expected_hash {
                return Err(TraceError::CheckpointMismatch {
                    tick: current_tick,
                    expected: checkpoint.expected_hash.clone(),
                    actual: hash,
                    label: checkpoint.label.clone(),
                });
            }
            next_checkpoint += 1;
            checkpoints_checked += 1;
        }
    }
    Ok(TraceRunReport {
        scenario_id: trace.scenario_id.clone(),
        seed: trace.seed,
        ticks_executed: trace.end_tick,
        commands_applied: cursor,
        final_state_hash: simulation.state_hash(),
        checkpoints_checked,
    })
}

#[derive(Debug)]
pub enum TraceError {
    InvalidTrace(String),
    Command {
        tick: u64,
        action: String,
        message: String,
    },
    CheckpointMismatch {
        tick: u64,
        expected: StateHash,
        actual: StateHash,
        label: Option<String>,
    },
    Encode(serde_json::Error),
    Decode(serde_json::Error),
}

impl fmt::Display for TraceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTrace(message) => write!(formatter, "invalid trace: {message}"),
            Self::Command {
                tick,
                action,
                message,
            } => write!(
                formatter,
                "command '{action}' failed at tick {tick}: {message}"
            ),
            Self::CheckpointMismatch {
                tick,
                expected,
                actual,
                label,
            } => {
                if let Some(label) = label {
                    write!(
                        formatter,
                        "trace checkpoint '{label}' failed at tick {tick}: expected {expected}, got {actual}"
                    )
                } else {
                    write!(
                        formatter,
                        "trace checkpoint failed at tick {tick}: expected {expected}, got {actual}"
                    )
                }
            }
            Self::Encode(error) => write!(formatter, "could not encode trace data: {error}"),
            Self::Decode(error) => write!(formatter, "could not decode trace data: {error}"),
        }
    }
}

impl std::error::Error for TraceError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct CounterSimulation {
        tick: u64,
        value: i64,
    }

    impl DeterministicSimulation for CounterSimulation {
        fn apply_command(&mut self, command: &SemanticCommand) -> Result<(), String> {
            match command.action.as_str() {
                "counter.add" => {
                    self.value += command
                        .payload
                        .as_i64()
                        .ok_or_else(|| "payload must be an integer".to_owned())?;
                    Ok(())
                }
                action => Err(format!("unknown action {action}")),
            }
        }

        fn fixed_step(&mut self) {
            self.tick += 1;
        }

        fn state_hash(&self) -> StateHash {
            hash_serializable(self).expect("counter state is serializable")
        }
    }

    #[test]
    fn trace_round_trips_and_replays_deterministically() {
        let mut trace = AuroraTrace::new("engine.counter", 44117, 60, 8);
        trace.push(
            SemanticCommand::new(1, "counter.add")
                .with_payload(&4_i64)
                .unwrap(),
        );
        trace.push(
            SemanticCommand::new(5, "counter.add")
                .with_payload(&-2_i64)
                .unwrap(),
        );
        let encoded = trace.to_json_pretty().unwrap();
        let decoded = AuroraTrace::from_json(&encoded).unwrap();

        let first = run_trace(&mut CounterSimulation { tick: 0, value: 0 }, &decoded).unwrap();
        let second = run_trace(&mut CounterSimulation { tick: 0, value: 0 }, &decoded).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.commands_applied, 2);
        assert_eq!(first.ticks_executed, 8);
    }

    #[test]
    fn malformed_or_unsorted_traces_are_rejected() {
        let mut trace = AuroraTrace::new("engine.counter", 1, 60, 4);
        trace.push(SemanticCommand::new(2, "counter.add"));
        trace.push(SemanticCommand::new(1, "counter.add"));
        assert!(matches!(trace.validate(), Err(TraceError::InvalidTrace(_))));

        trace.commands.clear();
        trace.push(SemanticCommand::new(4, "counter.add"));
        assert!(matches!(trace.validate(), Err(TraceError::InvalidTrace(_))));
    }

    #[test]
    fn checkpoints_verify_replay_state_at_named_ticks() {
        let trace = AuroraTrace::new("engine.counter", 7, 60, 4);
        let mut expected_sim = CounterSimulation { tick: 0, value: 0 };
        expected_sim.fixed_step();
        expected_sim.fixed_step();
        let expected_hash = expected_sim.state_hash();

        let report = run_trace_with_checkpoints(
            &mut CounterSimulation { tick: 0, value: 0 },
            &trace,
            &[TraceCheckpoint::new(
                2,
                expected_hash,
                Some("after two fixed ticks"),
            )],
        )
        .expect("checkpoint should match");

        assert_eq!(report.ticks_executed, 4);
        assert_eq!(report.checkpoints_checked, 1);
    }

    #[test]
    fn checkpoint_mismatch_is_a_diagnostic_error() {
        let trace = AuroraTrace::new("engine.counter", 7, 60, 4);
        let bad_checkpoint = TraceCheckpoint::new(1, StateHash(0), None);

        let mismatch = run_trace_with_checkpoints(
            &mut CounterSimulation { tick: 0, value: 0 },
            &trace,
            &[bad_checkpoint],
        )
        .expect_err("checkpoint should fail");

        assert!(matches!(
            mismatch,
            TraceError::CheckpointMismatch { tick: 1, .. }
        ));
    }

    #[test]
    fn byte_hash_is_stable_and_sensitive_to_order() {
        let mut first = StableStateHasher::new();
        first.write_u64(7);
        first.write_bool(true);
        let mut second = StableStateHasher::new();
        second.write_u64(7);
        second.write_bool(true);
        let mut reversed = StableStateHasher::new();
        reversed.write_bool(true);
        reversed.write_u64(7);

        assert_eq!(first.finish(), second.finish());
        assert_ne!(first.finish(), reversed.finish());
    }

    #[test]
    fn trace_journal_records_state_at_each_fixed_tick() {
        let mut trace = AuroraTrace::new("engine.counter", 11, 60, 4);
        trace.push(
            SemanticCommand::new(1, "counter.add")
                .with_payload(&5_i64)
                .unwrap(),
        );

        let mut journal = TraceRunJournal::new();
        let report = run_trace_with_checkpoints_and_journal(
            &mut CounterSimulation { tick: 0, value: 0 },
            &trace,
            &[],
            Some(&mut journal),
        )
        .unwrap();

        assert_eq!(journal.len(), 4);
        assert!(!journal.is_empty());
        assert_eq!(journal.records[0].tick, 1);
        assert_eq!(journal.records[0].fixed_tick, 1);
        // The command is scheduled at tick 1 and therefore applied before
        // the step that produces state 2; the state-1 record still shows a
        // clean pre-application count.
        assert_eq!(journal.records[0].commands_applied, 0);
        assert_eq!(journal.records[1].commands_applied, 1);
        assert_eq!(journal.final_hash(), Some(report.final_state_hash));
    }
}
