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
    #[test]
    fn game_owned_save_payload_stays_out_of_engine() {
        let save = SaveEnvelope::new(SAVE_VERSION, LabSave { ticks: 42 });
        assert_eq!(save.payload.ticks, 42);
    }
}
