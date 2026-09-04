//! Last Light's campaign-owned save schema and migration boundary.

use aurora_engine::{SaveEnvelope, SaveError, SaveStore};
use serde::{Deserialize, Serialize};

pub const SAVE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecialistLoadout {
    pub specialist: String,
    pub module: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CampaignProgress {
    pub unlocked_mission: u32,
    pub completed_missions: Vec<String>,
    pub currency: u64,
    pub decisions: Vec<String>,
    #[serde(default)]
    pub upgrades: Vec<String>,
    #[serde(default)]
    pub specialist_loadouts: Vec<SpecialistLoadout>,
}
impl Default for CampaignProgress {
    fn default() -> Self {
        Self {
            unlocked_mission: 1,
            completed_missions: vec![],
            currency: 0,
            decisions: vec![],
            upgrades: vec![],
            specialist_loadouts: vec![],
        }
    }
}
impl CampaignProgress {
    pub fn complete_mission(&mut self, id: impl Into<String>, unlock: u32, reward: u64) -> bool {
        let id = id.into();
        let new = !self.completed_missions.contains(&id);
        if new {
            self.completed_missions.push(id);
            self.currency = self.currency.saturating_add(reward);
        }
        self.unlocked_mission = self.unlocked_mission.max(unlock.max(1));
        new
    }
    pub fn record_decision(&mut self, decision: impl Into<String>) -> bool {
        let decision = decision.into();
        if self.decisions.contains(&decision) {
            false
        } else {
            self.decisions.push(decision);
            true
        }
    }
    pub fn has_decision(&self, decision: &str) -> bool {
        self.decisions.iter().any(|item| item == decision)
    }
    pub fn purchase_upgrade(&mut self, upgrade: impl Into<String>, cost: u64) -> bool {
        let upgrade = upgrade.into();
        if upgrade.trim().is_empty() || self.upgrades.contains(&upgrade) || self.currency < cost {
            return false;
        }
        self.currency -= cost;
        self.upgrades.push(upgrade);
        true
    }
    pub fn has_upgrade(&self, upgrade: &str) -> bool {
        self.upgrades.iter().any(|item| item == upgrade)
    }
    pub fn equip_specialist(
        &mut self,
        specialist: impl Into<String>,
        module: impl Into<String>,
    ) -> bool {
        let specialist = specialist.into();
        let module = module.into();
        if specialist.trim().is_empty() || module.trim().is_empty() {
            return false;
        }
        if let Some(loadout) = self
            .specialist_loadouts
            .iter_mut()
            .find(|item| item.specialist == specialist)
        {
            if loadout.module == module {
                return false;
            }
            loadout.module = module;
            return true;
        }
        self.specialist_loadouts
            .push(SpecialistLoadout { specialist, module });
        true
    }
    pub fn specialist_module<'a>(&'a self, specialist: &str, default: &'a str) -> &'a str {
        self.specialist_loadouts
            .iter()
            .find(|item| item.specialist == specialist)
            .map(|item| item.module.as_str())
            .unwrap_or(default)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SaveData {
    pub runs_completed: u32,
    #[serde(default)]
    pub campaign: CampaignProgress,
}

pub type CampaignStore = SaveStore<SaveData>;
/// Convenience loader retained for tooling that only needs the payload.
#[allow(dead_code)]
pub fn load(store: &CampaignStore) -> Result<Option<SaveData>, SaveError> {
    store
        .load_with(SAVE_VERSION, Ok)
        .map(|save| save.map(|envelope| envelope.payload))
}
pub fn envelope(data: SaveData) -> SaveEnvelope<SaveData> {
    SaveEnvelope::new(SAVE_VERSION, data)
}
