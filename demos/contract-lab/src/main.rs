//! A small public-API proof for Aurora's game ownership boundaries.

use aurora_engine::{
    run, ActionId, Color, FrameCtx, Game, InputMap, KeyBinding, SaveEnvelope, SaveStore,
};
use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

const SAVE_VERSION: u32 = 1;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct LabSave {
    ticks: u64,
}

struct ContractLab {
    bindings: InputMap,
    save: LabSave,
    store: SaveStore<LabSave>,
}

/// A deliberately tiny renderer-free game state proving Aurora can replay
/// semantic intent without knowing the game's command vocabulary.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LabSimulation {
    tick: u64,
    selected: bool,
    position: [i32; 2],
    salvage: u32,
}

#[cfg(test)]
impl LabSimulation {
    fn new(seed: u64) -> Self {
        Self {
            tick: 0,
            selected: false,
            position: [seed as i32 % 3, 0],
            salvage: 0,
        }
    }
}

#[cfg(test)]
impl aurora_engine::DeterministicSimulation for LabSimulation {
    fn apply_command(&mut self, command: &aurora_engine::SemanticCommand) -> Result<(), String> {
        match command.action.as_str() {
            "lab.select" => {
                self.selected = true;
                Ok(())
            }
            "lab.move" if self.selected => {
                self.position = serde_json::from_value(command.payload.clone())
                    .map_err(|error| format!("invalid destination: {error}"))?;
                Ok(())
            }
            "lab.harvest" if self.selected => {
                let amount: u32 = serde_json::from_value(command.payload.clone())
                    .map_err(|error| format!("invalid salvage amount: {error}"))?;
                self.salvage = self.salvage.saturating_add(amount);
                Ok(())
            }
            "lab.move" | "lab.harvest" => Err("no unit selected".to_owned()),
            action => Err(format!("unknown lab action {action}")),
        }
    }

    fn fixed_step(&mut self) {
        self.tick += 1;
    }

    fn state_hash(&self) -> aurora_engine::StateHash {
        aurora_engine::hash_serializable(self)
            .expect("lab simulation state has deterministic fields")
    }
}

impl ContractLab {
    fn new() -> Self {
        let mut bindings = InputMap::default();
        bindings.bind_key(
            ActionId::new("contract_lab.advance"),
            KeyBinding::key(KeyCode::Space),
        );
        Self {
            bindings,
            save: LabSave::default(),
            store: SaveStore::new("contract-lab", "default"),
        }
    }
}

impl Game for ContractLab {
    fn on_fixed_update(&mut self, _ctx: &mut FrameCtx<'_>) {
        self.save.ticks = self.save.ticks.saturating_add(1);
    }
    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        ctx.renderer.set_clear_color(Color::rgb(0.035, 0.06, 0.11));
        if ctx
            .input
            .action_pressed(&self.bindings, &ActionId::new("contract_lab.advance"))
        {
            let _ = self
                .store
                .save(&SaveEnvelope::new(SAVE_VERSION, self.save.clone()));
        }
    }
}

fn main() {
    run(ContractLab::new());
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_engine::{run_trace, AuroraTrace, SemanticCommand};
    #[test]
    fn game_owned_save_payload_stays_out_of_engine() {
        let save = SaveEnvelope::new(SAVE_VERSION, LabSave { ticks: 42 });
        assert_eq!(save.payload.ticks, 42);
    }

    #[test]
    fn game_owned_commands_replay_to_the_same_state_hash() {
        let mut trace = AuroraTrace::new("contract_lab.selection_move_harvest", 44117, 60, 12);
        trace.push(SemanticCommand::new(1, "lab.select"));
        trace.push(
            SemanticCommand::new(3, "lab.move")
                .with_payload(&[24_i32, -8_i32])
                .unwrap(),
        );
        trace.push(
            SemanticCommand::new(8, "lab.harvest")
                .with_payload(&15_u32)
                .unwrap(),
        );

        let first = run_trace(&mut LabSimulation::new(trace.seed), &trace).unwrap();
        let second = run_trace(&mut LabSimulation::new(trace.seed), &trace).unwrap();

        assert_eq!(first.final_state_hash, second.final_state_hash);
        assert_eq!(first.commands_applied, 3);
        assert_eq!(first.ticks_executed, 12);
    }
}
