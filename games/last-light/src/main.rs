//! Aurora: Last Light — Reclaim the Reactor.
//! Point-and-click RTS vertical slice powered by Aurora Engine.

mod assets;
mod campaign;
mod mission_state;
mod missions;
mod save;
mod simulation;
mod units;

use std::collections::{HashMap, VecDeque};

use assets::TextureAsset;
use aurora_engine::{
    run, Aabb, AiParams, AnimationClip, AnimationPlayer, BitmapText, Color, FogOfWar, FogState,
    FrameCtx, Game, MinimapTransform, PlacementError, PlacementRules, PointLight, PowerNodeId,
    QueueError, Renderer, SelectionBox, SimpleAggroAi, Sprite, TerrainClass, TerrainZone, Texture,
    TextureAtlas, TextureHandle, UnitId, UnitOrder,
};
use campaign::*;
use glam::Vec2;
use mission_state::{
    FieldBeacon, HarvestJob, HarvestPhase, ResourceKind, SalvageNode, SpecialistObjectiveKind,
    SpecialistObjectiveState, StructureKind, TerrainControlAdvance, TerrainControlObjective,
    TerrainControlPresence, TerrainControlState,
};
use missions::{DialogueTrigger, MissionDef, VictoryCondition};
use save::{CampaignStore, SaveData};
use simulation::{
    AbilityError, MissionOutcome, MissionSimulation, ProductionCancelCommandError,
    ProductionCommandError, RaidPhase, RaidState, SimulationEventKind, SimulationModifiers,
    SpecialAbility, StructureCommandError, FABRICATOR_NODE, MAP_SIZE, QUEUE_CANCEL_REFUND_PERCENT,
};
use units::{UnitKind, CHOIR, PLAYER};
use winit::{event::MouseButton, keyboard::KeyCode};

const BEACON_COST: u32 = 50;
/// One extracting Surveyor contributes this much deterministic resource per
/// second. Keeping the value beside the HUD formatter prevents the card from
/// promising a rate that differs from the fixed-step harvest loop.
const HARVEST_RATE_PER_SECOND: f32 = 18.0;
const ENEMY_CLICK_RADIUS_SCALE: f32 = 3.55;
const FRIENDLY_CLICK_RADIUS_SCALE: f32 = 1.9;
const FRIENDLY_HOVER_RADIUS_SCALE: f32 = 1.6;
const RESOURCE_CLICK_RADIUS: f32 = 130.0;
/// The shipped unit strips are authored facing the lower edge of the screen
/// (world-space -Y). Keep that art convention explicit so movement rotation
/// cannot silently turn every unit 180 degrees away from its travel direction.
const UNIT_ART_FORWARD_ANGLE: f32 = -std::f32::consts::FRAC_PI_2;
const COMMAND_CARD_KEYS: [KeyCode; 6] = [
    KeyCode::KeyQ,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyH,
    KeyCode::KeyT,
    KeyCode::KeyB,
];
/// Number of command rows shown at a time before paginating. A full command
/// surface shows PAGE_SIZE rows per page and cycles with arrow keys when the
/// surface has more than this many actions.
const COMMAND_CARD_COMPACT_ROWS: usize = 3;
const HUD_SCALE_MIN: f32 = 0.5;
const HUD_SCALE_MAX: f32 = 0.95;
/// At dense zoom levels the HUD should prioritize command clarity over
/// always-showing telemetry, so command and info overlays compress.
const HUD_DENSE_SCALE: f32 = 0.84;
const COMMAND_CARD_LABELS: [&str; 5] = [
    "Q  WARDEN  90",
    "E  ENGINEER  70",
    "F  SURVEYOR  60",
    "H  HOLD",
    "T  STOP",
];

/// Source atlas for a tactical unit card portrait.
///
/// Lantern specialists use the character portrait sheet because their named
/// callsigns are part of the campaign fiction. Choir contacts instead reuse
/// the authoritative six-cell unit atlas, whose lower row already contains
/// distinct Needle, Canticle, and Bell Mine silhouettes. Keeping this choice
/// explicit prevents enemy cards from silently falling back to Lumen's comms
/// portrait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitCardPortrait {
    Command(u32),
    Tactical(u32),
}

/// Presentation-only admission reasons for the Fabricator mini-menu.
/// `MissionSimulation::queue_unit` remains authoritative; this mirror lets
/// the card explain the same rejection before the player spends a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FabricatorBuildGate {
    Ready,
    UnitCap,
    Supply,
    Offline,
    Flux,
    QueueFull,
    Salvage,
}

/// Presentation fallback for structure states that do not yet have dedicated
/// animation strips. The simulation remains authoritative; this enum only
/// chooses a readable overlay from build progress, health, and power.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructureVisualState {
    Offline,
    Booting,
    Online,
    Damaged,
}

/// Rotate a Last Light unit strip so its authored front points along velocity.
///
/// A zero/near-zero velocity deliberately returns the neutral authored angle;
/// idle sprites should not jitter while pathfinding settles on a target.
fn unit_sprite_rotation(velocity: Vec2) -> f32 {
    if velocity.length_squared() <= 1.0 {
        0.0
    } else {
        velocity.y.atan2(velocity.x) - UNIT_ART_FORWARD_ANGLE
    }
}

/// Presentation state shared by animation playback and atlas selection.
/// Keeping one decision tree for both paths prevents a stale strip from
/// surviving a state change (for example, a Surveyor showing its scan fan
/// while merely walking to a node).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitAnimationState {
    Idle,
    Move,
    Attack,
    Repair,
    Build,
    Scan,
    Mark,
    Command,
    Arm,
    Hit,
    Down,
}

/// Result of applying a Fabricator rally to a newly deployed Surveyor.
///
/// Keeping the non-assignment cases explicit lets the HUD explain why a
/// worker stayed at the rally point (ordinary rally, dry node, or saturation)
/// instead of silently creating a job that the extraction loop cannot serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RallyHarvestOutcome {
    Assigned(usize),
    AlreadyAssigned(usize),
    Saturated(usize),
    Dry(usize),
    NotResource,
}

struct LastLight {
    tex_environment: TextureHandle,
    tex_units: TextureHandle,
    tex_warden_move: TextureHandle,
    tex_warden_attack: TextureHandle,
    tex_engineer_move: TextureHandle,
    tex_engineer_repair: TextureHandle,
    tex_engineer_build: TextureHandle,
    tex_surveyor_move: TextureHandle,
    tex_surveyor_scan: TextureHandle,
    tex_surveyor_mark: TextureHandle,
    tex_needle_attack: TextureHandle,
    tex_canticle_command: TextureHandle,
    tex_bell_mine_arm: TextureHandle,
    tex_hit_reactions: TextureHandle,
    tex_down_reactions: TextureHandle,
    tex_structures: TextureHandle,
    tex_portraits: TextureHandle,
    tex_mission_cover: TextureHandle,
    tex_resources: TextureHandle,
    tex_resource_effects: TextureHandle,
    tex_glow: TextureHandle,
    tex_ui: TextureHandle,
    unit_atlas: TextureAtlas,
    warden_move_atlas: TextureAtlas,
    warden_attack_atlas: TextureAtlas,
    engineer_move_atlas: TextureAtlas,
    engineer_repair_atlas: TextureAtlas,
    engineer_build_atlas: TextureAtlas,
    surveyor_move_atlas: TextureAtlas,
    surveyor_scan_atlas: TextureAtlas,
    surveyor_mark_atlas: TextureAtlas,
    needle_attack_atlas: TextureAtlas,
    canticle_command_atlas: TextureAtlas,
    bell_mine_arm_atlas: TextureAtlas,
    hit_reactions_atlas: TextureAtlas,
    down_reactions_atlas: TextureAtlas,
    animation_players: HashMap<UnitId, AnimationPlayer>,
    structure_atlas: TextureAtlas,
    portrait_atlas: TextureAtlas,
    resource_atlas: TextureAtlas,
    resource_effects_atlas: TextureAtlas,
    simulation: MissionSimulation,
    attack_flash: HashMap<UnitId, f32>,
    damage_flash: HashMap<UnitId, f32>,
    repair_flash: HashMap<UnitId, (UnitId, f32)>,
    build_flash: HashMap<UnitId, f32>,
    mark_flash: HashMap<UnitId, f32>,
    down_units: HashMap<UnitId, f32>,
    fog: FogOfWar,
    drag: Option<SelectionBox>,
    order_marker: Option<(Vec2, f32)>,
    salvage_nodes: Vec<SalvageNode>,
    harvest_jobs: HashMap<UnitId, HarvestJob>,
    lumen_cores: u32,
    reactor_position: Option<Vec2>,
    fabricator_position: Vec2,
    field_beacons: Vec<FieldBeacon>,
    placing_beacon: bool,
    attack_move_mode: bool,
    patrol_mode: bool,
    follow_mode: bool,
    status: Option<(String, f32)>,
    command_card_compact: bool,
    command_card_page: usize,
    minimal_hud: bool,
    save_store: CampaignStore,
    save_data: SaveData,
    victory_saved: bool,
    mission_select: bool,
    mission_cursor: usize,
    /// Set the first time `on_fixed_update` handles input each rendered
    /// frame, cleared once per frame in `on_update`. The fixed-step
    /// catch-up loop can run `on_fixed_update` several times in one
    /// rendered frame after a hitch, but `key_pressed`/`mouse_pressed` stay
    /// true for the whole frame — without this, a single click or keypress
    /// could fire twice (e.g. queuing two units off one click, or cascading
    /// through mission-select *and* straight out of the briefing). All
    /// edge-triggered input handling below is gated on this being false;
    /// continuous simulation (movement, combat, economy) still runs every
    /// fixed step regardless.
    input_handled_this_frame: bool,
    briefing: bool,
    paused: bool,
    victory: bool,
    defeat: bool,
    enemy_think: f32,
    mission_time: f32,
    mission: MissionDef,
    enemy_ai: SimpleAggroAi,
    selected_structure: Option<StructureKind>,
    /// Contextual selection for finite Salvage/Flux nodes. Resource objects
    /// use the same mini-menu surface as buildings without borrowing unit
    /// selection or changing worker extraction rules.
    selected_resource_node: Option<usize>,
    /// Progress for the mission-authored specialist beat. The state is kept
    /// presentation-side so the generic RTS simulation remains reusable while
    /// the campaign can still teach a role-specific job.
    specialist_objective_state: SpecialistObjectiveState,
    /// Campaign-owned terrain control progress. The generic simulation still
    /// owns movement/combat; this state only turns an authored high-ground
    /// occupation into a deterministic, readable mission beat.
    terrain_control_state: TerrainControlState,
    dialogue_cursor: usize,
    radio_message: Option<(&'static str, &'static str, f32)>,
    radio_queue: VecDeque<(&'static str, &'static str, Option<Vec2>)>,
    radio_priority_queue: VecDeque<(&'static str, &'static str, Option<Vec2>)>,
    radio_pop_in: f32,
    last_transmission: Option<Vec2>,
    /// A short-lived world-space cue for the most recent objective/comms
    /// target. Keeping this separate from the persistent `last_transmission`
    /// location lets Space continue to focus the inbox without leaving labels
    /// scattered across the playfield forever.
    target_feedback: Option<(Vec2, String, f32)>,
    /// The tactical view starts with a short onboarding hint, then keeps the
    /// playfield quiet. F1 can reopen it without permanently spending HUD
    /// space on a controls manual.
    controls_hint_remaining: f32,
}

impl LastLight {
    fn new() -> Self {
        let save_store = CampaignStore::new("last-light", "campaign");
        let save_data = save::load(&save_store).ok().flatten().unwrap_or_default();
        let starting_salvage = 150_u32.saturating_add(save_data.campaign.currency.min(100) as u32);
        let unlocked_tier = save_data.campaign.unlocked_mission;
        let mission_cursor = missions::all()
            .iter()
            .enumerate()
            .filter(|(_, mission)| unlocked_tier >= mission.required_tier)
            .map(|(index, _)| index)
            .next_back()
            .unwrap_or(0);
        let initial_mission = missions::reclaim_the_reactor();
        let initial_modifiers = SimulationModifiers {
            player_health: if save_data.campaign.has_upgrade(UPGRADE_PLATING) {
                1.2
            } else {
                1.0
            },
            player_speed: if save_data.campaign.specialist_module(MARA, MARA_RESCUE) == MARA_RAPID {
                1.12
            } else {
                1.0
            },
            starting_salvage,
            relay_income_per_second: 3,
            relay_restore_rate: if save_data.campaign.specialist_module(IVO, IVO_RIGGER)
                == IVO_RIGGER
            {
                1.5
            } else {
                1.0
            },
            production_time_scale: if save_data.campaign.has_upgrade(UPGRADE_OVERCLOCK) {
                0.75
            } else {
                1.0
            },
            player_damage_scale: 1.0,
            player_damage_taken_scale: 1.0,
        };
        let simulation = MissionSimulation::from_mission(&initial_mission, initial_modifiers);

        Self {
            tex_environment: TextureHandle::default(),
            tex_units: TextureHandle::default(),
            tex_warden_move: TextureHandle::default(),
            tex_warden_attack: TextureHandle::default(),
            tex_engineer_move: TextureHandle::default(),
            tex_engineer_repair: TextureHandle::default(),
            tex_engineer_build: TextureHandle::default(),
            tex_surveyor_move: TextureHandle::default(),
            tex_surveyor_scan: TextureHandle::default(),
            tex_surveyor_mark: TextureHandle::default(),
            tex_needle_attack: TextureHandle::default(),
            tex_canticle_command: TextureHandle::default(),
            tex_bell_mine_arm: TextureHandle::default(),
            tex_hit_reactions: TextureHandle::default(),
            tex_down_reactions: TextureHandle::default(),
            tex_structures: TextureHandle::default(),
            tex_portraits: TextureHandle::default(),
            tex_mission_cover: TextureHandle::default(),
            tex_resources: TextureHandle::default(),
            tex_resource_effects: TextureHandle::default(),
            tex_glow: TextureHandle::default(),
            tex_ui: TextureHandle::default(),
            unit_atlas: TextureAsset::Units.runtime_atlas(TextureHandle::default()),
            warden_move_atlas: TextureAsset::WardenMove.runtime_atlas(TextureHandle::default()),
            warden_attack_atlas: TextureAsset::WardenAttack.runtime_atlas(TextureHandle::default()),
            engineer_move_atlas: TextureAsset::EngineerMove.runtime_atlas(TextureHandle::default()),
            engineer_repair_atlas: TextureAsset::EngineerRepair
                .runtime_atlas(TextureHandle::default()),
            engineer_build_atlas: TextureAsset::EngineerBuild
                .runtime_atlas(TextureHandle::default()),
            surveyor_move_atlas: TextureAsset::SurveyorMove.runtime_atlas(TextureHandle::default()),
            surveyor_scan_atlas: TextureAsset::SurveyorScan.runtime_atlas(TextureHandle::default()),
            surveyor_mark_atlas: TextureAsset::SurveyorMark.runtime_atlas(TextureHandle::default()),
            needle_attack_atlas: TextureAsset::NeedleAttack.runtime_atlas(TextureHandle::default()),
            canticle_command_atlas: TextureAsset::CanticleCommand
                .runtime_atlas(TextureHandle::default()),
            bell_mine_arm_atlas: TextureAsset::BellMineArm.runtime_atlas(TextureHandle::default()),
            hit_reactions_atlas: TextureAsset::HitReactions.runtime_atlas(TextureHandle::default()),
            down_reactions_atlas: TextureAsset::DownReactions
                .runtime_atlas(TextureHandle::default()),
            animation_players: HashMap::new(),
            structure_atlas: TextureAsset::Structures.runtime_atlas(TextureHandle::default()),
            portrait_atlas: TextureAsset::CommandPortraits.runtime_atlas(TextureHandle::default()),
            resource_atlas: TextureAsset::ResourceNodes.runtime_atlas(TextureHandle::default()),
            resource_effects_atlas: TextureAsset::ResourceHarvestEffects
                .runtime_atlas(TextureHandle::default()),
            simulation,
            attack_flash: HashMap::new(),
            damage_flash: HashMap::new(),
            repair_flash: HashMap::new(),
            build_flash: HashMap::new(),
            mark_flash: HashMap::new(),
            down_units: HashMap::new(),
            fog: Self::new_fog(),
            drag: None,
            order_marker: None,
            salvage_nodes: Vec::new(),
            harvest_jobs: HashMap::new(),
            lumen_cores: 0,
            reactor_position: None,
            fabricator_position: Vec2::ZERO,
            field_beacons: Vec::new(),
            placing_beacon: false,
            attack_move_mode: false,
            patrol_mode: false,
            follow_mode: false,
            status: None,
            command_card_compact: true,
            command_card_page: 0,
            minimal_hud: false,
            save_store,
            save_data,
            victory_saved: false,
            mission_select: true,
            mission_cursor,
            input_handled_this_frame: false,
            briefing: false,
            paused: false,
            victory: false,
            defeat: false,
            enemy_think: 0.0,
            mission_time: 0.0,
            mission: initial_mission,
            enemy_ai: SimpleAggroAi::new(),
            selected_structure: None,
            selected_resource_node: None,
            specialist_objective_state: SpecialistObjectiveState::default(),
            terrain_control_state: TerrainControlState::default(),
            dialogue_cursor: 0,
            radio_message: None,
            radio_queue: VecDeque::new(),
            radio_priority_queue: VecDeque::new(),
            radio_pop_in: 0.0,
            last_transmission: None,
            target_feedback: None,
            controls_hint_remaining: 0.0,
        }
    }

    /// Resets all mission-scoped state (world, economy, power, nav) and
    /// spawns `mission`'s roster. Campaign-wide state (`save_data`, loaded
    /// textures/atlases) is left untouched.
    fn start_mission(&mut self, mission: MissionDef) {
        let modifiers = self.simulation_modifiers();
        self.simulation = MissionSimulation::from_mission(&mission, modifiers);
        self.mission = mission;
        self.command_card_page = 0;
        self.animation_players.clear();
        self.animation_players.extend(
            self.simulation
                .kinds
                .keys()
                .map(|id| (*id, AnimationPlayer::default())),
        );
        self.attack_flash.clear();
        self.damage_flash.clear();
        self.build_flash.clear();
        self.mark_flash.clear();
        self.down_units.clear();
        self.fog = Self::new_fog();
        self.drag = None;
        self.order_marker = None;
        self.salvage_nodes = self
            .mission
            .salvage_nodes
            .iter()
            .enumerate()
            .map(|(index, &position)| SalvageNode {
                position,
                remaining: 240,
                harvest_buffer: 0.0,
                kind: if index >= 3 {
                    ResourceKind::Flux
                } else {
                    ResourceKind::Salvage
                },
                max_workers: if index >= 3 { 3 } else { 2 },
            })
            .collect();
        self.harvest_jobs.clear();
        self.lumen_cores = 0;
        self.reactor_position = self.mission.reactor_position;
        self.fabricator_position = self.mission.fabricator_position;
        self.field_beacons.clear();
        self.placing_beacon = false;
        self.attack_move_mode = false;
        self.patrol_mode = false;
        self.follow_mode = false;
        self.enemy_ai = SimpleAggroAi::new();
        self.selected_structure = None;
        self.selected_resource_node = None;
        self.specialist_objective_state = SpecialistObjectiveState::default();
        self.terrain_control_state = TerrainControlState::default();
        self.dialogue_cursor = 0;
        self.radio_message = None;
        self.radio_queue.clear();
        self.radio_priority_queue.clear();
        self.radio_pop_in = 0.0;
        self.last_transmission = None;
        self.target_feedback = None;
        self.controls_hint_remaining = 1.5;

        self.victory_saved = false;
        self.briefing = true;
        self.paused = false;
        self.victory = false;
        self.defeat = false;
        self.enemy_think = 0.0;
        self.mission_time = 0.0;
        self.status = Some(("FABRICATOR READY".to_owned(), 3.0));

        // A mission opens with its field roster ready to command. This avoids
        // making the very first interaction depend on discovering sprites
        // beneath the tactical HUD or minimap.
        self.simulation.select_all_player_units();
    }

    fn new_fog() -> FogOfWar {
        const CELL: f32 = 100.0;
        FogOfWar::new(
            (MAP_SIZE.x / CELL).ceil() as usize,
            (MAP_SIZE.y / CELL).ceil() as usize,
            -MAP_SIZE * 0.5,
            CELL,
        )
    }

    fn simulation_modifiers(&self) -> SimulationModifiers {
        SimulationModifiers {
            // A clean mission starts with a small survivability buffer. The
            // first relay should teach formation and target priority, while
            // Reactive Plating remains a meaningful campaign upgrade.
            player_health: if self.save_data.campaign.has_upgrade(UPGRADE_PLATING) {
                1.2
            } else {
                1.08
            },
            player_speed: if self.specialist_module(MARA, MARA_RESCUE) == MARA_RAPID {
                1.12
            } else {
                1.0
            },
            // Salvage is tactical mission currency, not a hidden carry-over
            // from the last attempt. Resetting it makes retrying a defeat
            // fair and keeps the production curve authored per mission.
            starting_salvage: 150_u32
                .saturating_add(self.save_data.campaign.currency.min(100) as u32),
            relay_income_per_second: self.relay_income(),
            relay_restore_rate: if self.specialist_module(IVO, IVO_RIGGER) == IVO_RIGGER {
                1.5
            } else {
                1.0
            },
            production_time_scale: if self.save_data.campaign.has_upgrade(UPGRADE_OVERCLOCK) {
                0.75
            } else {
                1.0
            },
            player_damage_scale: if self.specialist_module(SENA, SENA_DEEP_SCAN) == SENA_GHOST_MARK
            {
                1.15
            } else {
                1.0
            } * if self.specialist_module(OLAN, OLAN_LATTICE) == OLAN_DECODER {
                1.1
            } else {
                1.0
            },
            player_damage_taken_scale: if self.meridian_accord() == Some(MERIDIAN_BASTION) {
                0.82
            } else {
                0.92
            },
        }
    }

    fn purchase_upgrade(&mut self, id: &'static str, label: &str, cost: u64) {
        if self.save_data.campaign.has_upgrade(id) {
            self.status = Some((format!("{label} ALREADY INSTALLED"), 2.5));
            return;
        }
        if !self.save_data.campaign.purchase_upgrade(id, cost) {
            self.status = Some((format!("{label} REQUIRES {cost} LUMEN"), 2.5));
            return;
        }
        self.status = Some(
            match self
                .save_store
                .save(&save::envelope(self.save_data.clone()))
            {
                Ok(()) => (format!("{label} INSTALLED"), 3.5),
                Err(error) => (format!("UPGRADE SAVE FAILED: {error}"), 5.0),
            },
        );
    }

    fn specialist_module<'a>(&'a self, specialist: &str, default: &'a str) -> &'a str {
        self.save_data
            .campaign
            .specialist_module(specialist, default)
    }

    fn cycle_specialist(
        &mut self,
        specialist: &'static str,
        first: &'static str,
        second: &'static str,
        label: &str,
    ) {
        let next = if self.specialist_module(specialist, first) == first {
            second
        } else {
            first
        };
        self.save_data.campaign.equip_specialist(specialist, next);
        self.status = Some(
            match self
                .save_store
                .save(&save::envelope(self.save_data.clone()))
            {
                Ok(()) => (format!("{label} LOADOUT: {}", next.to_uppercase()), 3.5),
                Err(error) => (format!("LOADOUT SAVE FAILED: {error}"), 5.0),
            },
        );
    }

    fn beacon_cost(&self) -> u32 {
        let smith_discount = u32::from(self.specialist_module(IVO, IVO_RIGGER) == IVO_SMITH) * 10;
        let charter_discount = u32::from(self.meridian_accord() == Some(MERIDIAN_CHARTER)) * 10;
        BEACON_COST.saturating_sub(smith_discount + charter_discount)
    }

    fn relay_income(&self) -> u32 {
        let lattice_bonus = u32::from(self.specialist_module(OLAN, OLAN_LATTICE) == OLAN_LATTICE);
        let witness_bonus = u32::from(self.lumen_protocol() == Some(LUMEN_WITNESS));
        let charter_bonus = u32::from(self.meridian_accord() == Some(MERIDIAN_CHARTER));
        3 + lattice_bonus + witness_bonus + charter_bonus
    }

    fn lumen_protocol(&self) -> Option<&str> {
        self.save_data
            .campaign
            .has_decision(LUMEN_CONTACT)
            .then(|| self.specialist_module(LUMEN, LUMEN_GUARDIAN))
    }

    fn cycle_lumen_protocol(&mut self) {
        if !self.save_data.campaign.has_decision(LUMEN_CONTACT) {
            self.status = Some(("LUMEN LINK LOCKED — RECLAIM THE REACTOR".to_owned(), 3.5));
            return;
        }
        self.cycle_specialist(LUMEN, LUMEN_GUARDIAN, LUMEN_WITNESS, "LUMEN");
    }

    fn relationship_module<'a>(
        &'a self,
        decision: &str,
        faction: &str,
        default: &'a str,
    ) -> Option<&'a str> {
        self.save_data
            .campaign
            .has_decision(decision)
            .then(|| self.specialist_module(faction, default))
    }

    fn meridian_accord(&self) -> Option<&str> {
        self.relationship_module(MERIDIAN_ALLIED, MERIDIAN, MERIDIAN_BASTION)
    }

    fn verdant_covenant(&self) -> Option<&str> {
        self.relationship_module(VERDANT_CULTIVATED, VERDANT, VERDANT_BLOOM)
    }

    fn cycle_relationship(
        &mut self,
        decision: &'static str,
        faction: &'static str,
        first: &'static str,
        second: &'static str,
        label: &str,
        locked: &str,
    ) {
        if !self.save_data.campaign.has_decision(decision) {
            self.status = Some((locked.to_owned(), 3.5));
            return;
        }
        self.cycle_specialist(faction, first, second, label);
    }

    fn unlocked_mission_indices(&self) -> Vec<usize> {
        missions::all()
            .iter()
            .enumerate()
            .filter(|(_, mission)| {
                self.save_data.campaign.unlocked_mission >= mission.required_tier
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn handle_mission_select(&mut self, ctx: &mut FrameCtx<'_>) {
        let unlocked = self.unlocked_mission_indices();
        if unlocked.is_empty() {
            return;
        }
        let cursor_slot = unlocked
            .iter()
            .position(|&index| index == self.mission_cursor)
            .unwrap_or(0);
        if ctx.input.key_pressed(KeyCode::ArrowUp) || ctx.input.key_pressed(KeyCode::ArrowLeft) {
            let previous = (cursor_slot + unlocked.len() - 1) % unlocked.len();
            self.mission_cursor = unlocked[previous];
        }
        if ctx.input.key_pressed(KeyCode::ArrowDown) || ctx.input.key_pressed(KeyCode::ArrowRight) {
            let next = (cursor_slot + 1) % unlocked.len();
            self.mission_cursor = unlocked[next];
        }
        let mut confirmed =
            ctx.input.key_pressed(KeyCode::Space) || ctx.input.key_pressed(KeyCode::Enter);
        if ctx.input.mouse_pressed(MouseButton::Left) {
            let mouse_world = ctx
                .renderer
                .camera
                .screen_to_world(ctx.input.mouse_position);
            for &index in &unlocked {
                let menu_scale =
                    Self::mission_select_scale(ctx.renderer.camera.visible_world_size());
                if Self::mission_entry_rect(ctx.renderer.camera.position, index, menu_scale)
                    .contains_point(mouse_world)
                {
                    self.mission_cursor = index;
                    confirmed = true;
                    break;
                }
            }
        }
        if confirmed {
            if let Some(chosen) = missions::all().into_iter().nth(self.mission_cursor) {
                self.mission_select = false;
                self.start_mission(chosen);
                ctx.audio.start();
            }
        }
    }

    const BRIEFING_ROW_KEYS: [KeyCode; 10] = [
        KeyCode::KeyZ,
        KeyCode::KeyX,
        KeyCode::KeyC,
        KeyCode::KeyV,
        KeyCode::KeyN,
        KeyCode::KeyM,
        KeyCode::KeyO,
        KeyCode::KeyL,
        KeyCode::KeyP,
        KeyCode::KeyG,
    ];

    fn apply_briefing_action(&mut self, key: KeyCode) {
        match key {
            KeyCode::KeyZ => self.purchase_upgrade(UPGRADE_OPTICS, "FIELD OPTICS", 60),
            KeyCode::KeyX => self.purchase_upgrade(UPGRADE_PLATING, "REACTIVE PLATING", 80),
            KeyCode::KeyC => self.purchase_upgrade(UPGRADE_OVERCLOCK, "FABRICATOR OVERCLOCK", 100),
            KeyCode::KeyV => self.cycle_specialist(IVO, IVO_RIGGER, IVO_SMITH, "IVO"),
            KeyCode::KeyN => self.cycle_specialist(SENA, SENA_DEEP_SCAN, SENA_GHOST_MARK, "SENA"),
            KeyCode::KeyM => self.cycle_specialist(MARA, MARA_RESCUE, MARA_RAPID, "MARA"),
            KeyCode::KeyO => self.cycle_specialist(OLAN, OLAN_LATTICE, OLAN_DECODER, "OLAN"),
            KeyCode::KeyL => self.cycle_lumen_protocol(),
            KeyCode::KeyP => self.cycle_relationship(
                MERIDIAN_ALLIED,
                MERIDIAN,
                MERIDIAN_BASTION,
                MERIDIAN_CHARTER,
                "MERIDIAN",
                "MERIDIAN ACCORD LOCKED — COMPLETE TERMS OF SALVAGE",
            ),
            KeyCode::KeyG => self.cycle_relationship(
                VERDANT_CULTIVATED,
                VERDANT,
                VERDANT_BLOOM,
                VERDANT_BRIAR,
                "VERDANT",
                "VERDANT COVENANT LOCKED — COMPLETE THE GARDEN BELOW",
            ),
            _ => {}
        }
    }

    /// One row per briefing action: the key that triggers it, its label,
    /// and the accent color drawn in `on_update`. A single source of truth
    /// for both rendering and click hit-testing, so they can't drift apart.
    fn briefing_rows(&self) -> Vec<(KeyCode, String, Color)> {
        let owned = |id: &str| {
            if self.save_data.campaign.has_upgrade(id) {
                "INSTALLED"
            } else {
                "AVAILABLE"
            }
        };
        // These labels are intentionally compact: the full lock reason is
        // still delivered by `apply_briefing_action` as a transient status,
        // while the briefing grid remains scannable at browser width.
        let lumen_protocol = self
            .lumen_protocol()
            .map(str::to_uppercase)
            .unwrap_or_else(|| "LOCKED".to_owned());
        let meridian_accord = self
            .meridian_accord()
            .map(str::to_uppercase)
            .unwrap_or_else(|| "LOCKED".to_owned());
        let verdant_covenant = self
            .verdant_covenant()
            .map(str::to_uppercase)
            .unwrap_or_else(|| "LOCKED".to_owned());
        vec![
            (
                KeyCode::KeyZ,
                format!("Z  FIELD OPTICS  60  // {}", owned(UPGRADE_OPTICS)),
                Color::rgba(0.55, 0.82, 0.88, 0.98),
            ),
            (
                KeyCode::KeyX,
                format!("X  REACTIVE PLATING  80  // {}", owned(UPGRADE_PLATING)),
                Color::rgba(0.55, 0.82, 0.88, 0.98),
            ),
            (
                KeyCode::KeyC,
                format!(
                    "C  FABRICATOR OVERCLOCK  100  // {}",
                    owned(UPGRADE_OVERCLOCK)
                ),
                Color::rgba(0.55, 0.82, 0.88, 0.98),
            ),
            (
                KeyCode::KeyV,
                format!(
                    "V  IVO  // {}",
                    self.specialist_module(IVO, IVO_RIGGER).to_uppercase()
                ),
                Color::rgba(0.82, 0.68, 0.36, 0.98),
            ),
            (
                KeyCode::KeyN,
                format!(
                    "N  SENA  // {}",
                    self.specialist_module(SENA, SENA_DEEP_SCAN).to_uppercase()
                ),
                Color::rgba(0.82, 0.68, 0.36, 0.98),
            ),
            (
                KeyCode::KeyM,
                format!(
                    "M  MARA  // {}",
                    self.specialist_module(MARA, MARA_RESCUE).to_uppercase()
                ),
                Color::rgba(0.7, 0.62, 0.9, 0.98),
            ),
            (
                KeyCode::KeyO,
                format!(
                    "O  OLAN  // {}",
                    self.specialist_module(OLAN, OLAN_LATTICE).to_uppercase()
                ),
                Color::rgba(0.7, 0.62, 0.9, 0.98),
            ),
            (
                KeyCode::KeyL,
                format!("L  LUMEN  // {lumen_protocol}"),
                Color::rgba(0.38, 0.9, 1.0, 0.98),
            ),
            (
                KeyCode::KeyP,
                format!("P  MERIDIAN  // {meridian_accord}"),
                Color::rgba(0.9, 0.82, 0.72, 0.98),
            ),
            (
                KeyCode::KeyG,
                format!("G  VERDANT  // {verdant_covenant}"),
                Color::rgba(0.48, 1.15, 0.5, 0.98),
            ),
        ]
    }

    /// Keeps the longest upgrade label inside its fixed two-column row. The
    /// row hitbox stays constant so pointer targeting and keyboard bindings do
    /// not change when a campaign save swaps READY for INSTALLED.
    fn briefing_label_scale(label: &str, overlay_scale: f32) -> f32 {
        let character_count = label.chars().count() as f32;
        let fit_scale = (40.0 / character_count.max(40.0)).clamp(0.82, 1.0);
        1.8 * fit_scale * overlay_scale
    }

    fn briefing_row_rect(camera_position: Vec2, index: usize, scale: f32) -> Aabb {
        // Two columns keep the ten actions visible without turning the
        // briefing into a scrolling wall of text. Five rows per column also
        // leaves a clean lower band for the deploy prompt and the speaker
        // portrait card.
        let column = index / 5;
        let row = index % 5;
        let center = camera_position
            + Vec2::new(-120.0 + column as f32 * 420.0, 36.0 - row as f32 * 40.0) * scale;
        Aabb::from_center_size(center, Vec2::new(392.0, 34.0) * scale)
    }

    fn handle_briefing_upgrades(&mut self, ctx: &mut FrameCtx<'_>) {
        for key in Self::BRIEFING_ROW_KEYS {
            if ctx.input.key_pressed(key) {
                self.apply_briefing_action(key);
            }
        }
        if ctx.input.mouse_pressed(MouseButton::Left) {
            let mouse_world = ctx
                .renderer
                .camera
                .screen_to_world(ctx.input.mouse_position);
            let row_count = self.briefing_rows().len();
            let scale = Self::hud_scale(ctx.renderer);
            for index in 0..row_count {
                if Self::briefing_row_rect(ctx.renderer.camera.position, index, scale)
                    .contains_point(mouse_world)
                {
                    let key = self.briefing_rows()[index].0;
                    self.apply_briefing_action(key);
                    break;
                }
            }
        }
    }

    fn placement_rules(&self) -> PlacementRules {
        let mut power_sources = vec![self.fabricator_position];
        power_sources.extend(
            self.simulation
                .relays
                .iter()
                .filter(|relay| relay.active)
                .map(|relay| relay.position),
        );
        power_sources.extend(self.field_beacons.iter().map(|beacon| beacon.position));
        let mut obstructions = vec![(self.fabricator_position, 105.0)];
        if let Some(reactor_position) = self.reactor_position {
            obstructions.push((reactor_position, 135.0));
        }
        obstructions.extend(
            self.simulation
                .relays
                .iter()
                .map(|relay| (relay.position, 85.0)),
        );
        obstructions.extend(
            self.field_beacons
                .iter()
                .map(|beacon| (beacon.position, 65.0)),
        );
        PlacementRules {
            build_area: Aabb::from_center_size(Vec2::ZERO, MAP_SIZE - Vec2::splat(80.0)),
            power_sources,
            obstructions,
            max_power_distance: 470.0,
        }
    }

    fn minimap_transform(&self, renderer: &Renderer) -> MinimapTransform {
        let scale = Self::hud_scale(renderer);
        let bottom_left = renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(0.0, 0.0));
        MinimapTransform {
            world: Aabb::from_center_size(Vec2::ZERO, MAP_SIZE),
            panel: Aabb::from_center_size(
                bottom_left + Vec2::new(150.0, 92.0) * scale,
                Vec2::new(260.0, 138.0) * scale,
            ),
        }
    }

    fn hud_scale_for_view(view: Vec2, zoom: f32) -> f32 {
        const REFERENCE_VIEW: Vec2 = Vec2::from_array([1164.0, 654.0]);
        let zoom_scale = zoom.max(f32::EPSILON).recip();
        let view_scale = (view.x / REFERENCE_VIEW.x).min(view.y / REFERENCE_VIEW.y);
        (zoom_scale * view_scale).clamp(HUD_SCALE_MIN, HUD_SCALE_MAX)
    }

    fn hud_dense_layout(renderer: &Renderer) -> bool {
        Self::hud_scale(renderer) >= HUD_DENSE_SCALE
    }

    fn should_auto_minimize_hud(renderer: &Renderer) -> bool {
        Self::hud_dense_layout(renderer)
    }

    fn hud_scale(renderer: &Renderer) -> f32 {
        Self::hud_scale_for_view(renderer.camera.visible_world_size(), renderer.camera.zoom)
    }

    fn controls_hint_visible(&self) -> bool {
        self.controls_hint_remaining > 0.0 && self.radio_message.is_none()
    }

    fn queue_unit(&mut self, kind: UnitKind) {
        match self.simulation.queue_unit(kind) {
            Ok(()) => {
                self.status = Some((format!("{} ADDED TO QUEUE", kind.label()), 2.5));
            }
            Err(ProductionCommandError::UnitCap) => {
                self.status = Some(("UNIT CAP 12".to_owned(), 2.5));
            }
            Err(ProductionCommandError::SupplyBlocked) => {
                self.status = Some(("SUPPLY BLOCKED — BUILD CAPACITY".to_owned(), 2.5));
            }
            Err(ProductionCommandError::FabricatorOffline) => {
                self.status = Some(("FABRICATOR OFFLINE".to_owned(), 2.5));
            }
            Err(ProductionCommandError::UnsupportedUnit) => {}
            Err(ProductionCommandError::InsufficientFlux) => {
                self.status = Some(("INSUFFICIENT FLUX — HARVEST BLUE NODES".to_owned(), 2.5));
            }
            Err(ProductionCommandError::Queue(QueueError::InsufficientResources)) => {
                self.status = Some(("INSUFFICIENT SALVAGE".to_owned(), 2.5));
            }
            Err(ProductionCommandError::Queue(QueueError::Full)) => {
                self.status = Some(("PRODUCTION QUEUE FULL".to_owned(), 2.5));
            }
        }
    }

    fn upgrade_supply_module(&mut self) {
        match self.simulation.queue_supply_module() {
            Ok(()) => {
                self.status = Some(("SUPPLY MODULE CONSTRUCTION STARTED // 6s".to_owned(), 3.0));
            }
            Err(StructureCommandError::FabricatorOffline) => {
                self.status = Some(("SUPPLY MODULE // FABRICATOR OFFLINE".to_owned(), 2.5));
            }
            Err(StructureCommandError::Busy) => {
                self.status = Some(("SUPPLY MODULE // BUILD ALREADY IN PROGRESS".to_owned(), 2.5));
            }
            Err(StructureCommandError::Maxed) => {
                self.status = Some(("SUPPLY MODULE MAXED 24/24".to_owned(), 2.5));
            }
            Err(StructureCommandError::InsufficientResources) => {
                self.status = Some(("SUPPLY MODULE REQUIRES 100 SALVAGE".to_owned(), 2.5));
            }
        }
    }

    /// Cancels the selected Fabricator queue slot through the simulation's
    /// atomic refund path. The HUD never reconstructs costs; it only formats
    /// the receipt returned by the engine-backed command.
    fn cancel_queued_unit(&mut self, index: usize) {
        match self.simulation.cancel_queued_unit(index) {
            Ok(receipt) => {
                self.status = Some((
                    format!(
                        "{} CANCELLED // +{} SALVAGE +{} FLUX // SUPPLY +{}",
                        receipt.kind.label(),
                        receipt.refunded_salvage,
                        receipt.refunded_flux,
                        receipt.released_supply
                    ),
                    3.0,
                ));
            }
            Err(ProductionCancelCommandError::FabricatorOffline) => {
                self.status = Some(("CANCEL BLOCKED // FABRICATOR OFFLINE".to_owned(), 2.5));
            }
            Err(ProductionCancelCommandError::InvalidIndex) => {
                self.status = Some(("CANCEL BLOCKED // QUEUE EMPTY".to_owned(), 2.5));
            }
            Err(ProductionCancelCommandError::UnsupportedUnit) => {
                self.status = Some(("CANCEL BLOCKED // UNKNOWN QUEUE ITEM".to_owned(), 2.5));
            }
            Err(ProductionCancelCommandError::SupplyLedgerRequired) => {
                self.status = Some(("CANCEL BLOCKED // SUPPLY LEDGER ERROR".to_owned(), 2.5));
            }
        }
    }

    fn selected_unit_kind(&self) -> Option<UnitKind> {
        let mut kinds = self
            .simulation
            .world
            .selection()
            .ids()
            .iter()
            .filter_map(|id| {
                self.simulation
                    .world
                    .unit(*id)
                    .filter(|unit| unit.faction == PLAYER && unit.alive())
                    .and_then(|_| self.simulation.kinds.get(id).copied())
            });
        let first = kinds.next()?;
        kinds.all(|kind| kind == first).then_some(first)
    }

    fn selected_single_unit_kind(&self) -> Option<UnitKind> {
        (self.simulation.world.selection().ids().len() == 1)
            .then(|| self.selected_unit_kind())
            .flatten()
    }

    fn selected_unit_id(&self) -> Option<UnitId> {
        self.simulation
            .world
            .selection()
            .ids()
            .iter()
            .find(|id| {
                self.simulation
                    .world
                    .unit(**id)
                    .is_some_and(|unit| unit.faction == PLAYER && unit.alive())
            })
            .copied()
    }

    fn selected_squad_active(&self) -> bool {
        self.simulation.world.selection().ids().iter().any(|id| {
            self.simulation
                .world
                .unit(*id)
                .is_some_and(|unit| unit.faction == PLAYER && unit.alive())
        })
    }

    /// The command card is a contextual surface, not a permanent toolbar.
    /// Keyboard production shortcuts remain available with an empty
    /// selection, but the large panel should only occupy the playfield when
    /// it has a selected unit, structure, or resource node to describe.
    fn command_card_visible(&self) -> bool {
        self.selected_structure.is_some()
            || self.selected_resource_node.is_some()
            || self.selected_squad_active()
    }

    /// Command card rows should remain stable across contexts; this helper
    /// always returns only non-empty rows so hidden rows do not render as dead
    /// space.
    fn command_card_rows(&self) -> Vec<usize> {
        (0..COMMAND_CARD_KEYS.len())
            .filter(|&index| {
                let key = self.command_card_key(index);
                let label = self.command_card_label(index);
                key.is_some() || !label.is_empty()
            })
            .collect()
    }

    fn command_card_page_count(&self) -> usize {
        let rows = self.command_card_rows().len().max(1);
        (rows + COMMAND_CARD_COMPACT_ROWS - 1) / COMMAND_CARD_COMPACT_ROWS
    }

    fn command_card_visible_page(&self) -> usize {
        let max_page = self.command_card_page_count().saturating_sub(1);
        self.command_card_page.min(max_page)
    }

    fn command_card_should_paginate(&self, renderer: &Renderer) -> bool {
        self.command_card_visible()
            && !self.command_card_compact
            && !self.minimal_hud
            && !Self::hud_dense_layout(renderer)
            && self.command_card_rows().len() > COMMAND_CARD_COMPACT_ROWS
    }

    /// Compact mode keeps the command card readable in browser zooms by
    /// restricting to the first few actionable rows.
    #[allow(dead_code)]
    fn visible_command_card_rows(&self) -> Vec<usize> {
        let rows = self.command_card_rows();
        if !self.command_card_compact && !self.minimal_hud {
            let start = self.command_card_visible_page() * COMMAND_CARD_COMPACT_ROWS;
            rows.into_iter()
                .skip(start)
                .take(COMMAND_CARD_COMPACT_ROWS)
                .collect()
        } else {
            rows.into_iter().take(COMMAND_CARD_COMPACT_ROWS).collect()
        }
    }

    fn visible_command_card_rows_for_display(&self, renderer: &Renderer) -> Vec<usize> {
        let rows = self.command_card_rows();
        if self.command_card_compact || self.minimal_hud || Self::hud_dense_layout(renderer) {
            rows.into_iter().take(COMMAND_CARD_COMPACT_ROWS).collect()
        } else {
            let start = self.command_card_visible_page() * COMMAND_CARD_COMPACT_ROWS;
            rows.into_iter()
                .skip(start)
                .take(COMMAND_CARD_COMPACT_ROWS)
                .collect()
        }
    }

    #[allow(dead_code)]
    fn command_card_has_more_rows(&self) -> bool {
        self.command_card_rows().len() > COMMAND_CARD_COMPACT_ROWS
    }

    fn clamp_command_card_page_to_context(&mut self) {
        let max_page = self.command_card_page_count().saturating_sub(1);
        if self.command_card_page > max_page {
            self.command_card_page = max_page;
        }
    }

    fn reset_command_card_page(&mut self) {
        self.command_card_page = 0;
    }

    fn next_command_card_page(&mut self, renderer: &Renderer) {
        if !self.command_card_should_paginate(renderer) {
            return;
        }
        self.command_card_page =
            (self.command_card_visible_page() + 1) % self.command_card_page_count();
        self.status = Some((
            format!(
                "CMD PAGE {} / {}",
                self.command_card_visible_page() + 1,
                self.command_card_page_count()
            ),
            1.4,
        ));
    }

    fn prev_command_card_page(&mut self, renderer: &Renderer) {
        if !self.command_card_should_paginate(renderer) {
            return;
        }
        let page_count = self.command_card_page_count();
        let page = self.command_card_visible_page();
        self.command_card_page = if page == 0 { page_count - 1 } else { page - 1 };
        self.status = Some((
            format!(
                "CMD PAGE {} / {}",
                self.command_card_visible_page() + 1,
                page_count
            ),
            1.4,
        ));
    }

    /// Structure and resource-node cards are mutually exclusive contexts.
    /// Pointer selection already clears the other field, but normalizing at
    /// the frame boundary protects the renderer from stale state after a
    /// menu transition or a future selection path.
    fn normalize_selection_context(&mut self) {
        let structure_selected = self.selected_structure.is_some();
        let resource_index_invalid = self
            .selected_resource_node
            .is_some_and(|node| node >= self.salvage_nodes.len());
        if structure_selected || resource_index_invalid {
            self.selected_resource_node = None;
        }
    }

    /// Keep mixed-selection telemetry legible without leaving tutorial copy
    /// over the playfield after the opening handoff. The command card remains
    /// the persistent source of actionable verbs; this line is only a compact
    /// roster readout once onboarding has expired.
    fn mixed_squad_role_line(&self, role_counts: [u32; 3]) -> String {
        let counts = format!(
            "W{}  E{}  S{}",
            role_counts[0], role_counts[1], role_counts[2]
        );
        if self.controls_hint_remaining > 0.0 {
            format!("{counts}   // CLICK PORTRAIT TO SPLIT")
        } else {
            counts
        }
    }

    fn unit_identity_label(&self, id: UnitId, kind: UnitKind) -> String {
        self.simulation
            .callsign(id)
            .unwrap_or(kind.label())
            .to_owned()
    }

    /// Compact terrain context for the selected unit card. This uses the
    /// engine resolver rather than re-reading authored floats in the HUD, so
    /// combat cover, minimap overlays, and player-facing copy stay in lockstep.
    fn terrain_readout_copy(&self, position: Vec2) -> Option<(String, Color)> {
        // The engine returns `None` outside authored zones; for a player-facing
        // card that is still meaningful terrain, so expose it as neutral open
        // ground rather than making the HUD disappear.
        let readout = TerrainZone::resolve_readout_at(position, &self.mission.terrain_zones)
            .map(|(_, readout)| readout)
            .unwrap_or(aurora_engine::TerrainReadout {
                class: TerrainClass::Open,
                elevation: 0,
                cover_percent: 0,
            });
        let (label, accent) = match readout.class {
            TerrainClass::Open => ("OPEN", Color::rgba(0.58, 0.72, 0.75, 0.86)),
            TerrainClass::Covered => ("COVER", Color::rgba(0.76, 0.5, 1.2, 0.92)),
            TerrainClass::HighGround => ("HIGH", Color::rgba(0.28, 1.3, 1.2, 0.92)),
            TerrainClass::FortifiedHighGround => ("FORTIFIED", Color::rgba(1.15, 0.76, 0.28, 0.94)),
        };
        Some((
            format!("TERRAIN {label} // COVER {:02}%", readout.cover_percent),
            accent,
        ))
    }

    /// Returns a cover-sized world sprite for the authored sector plate.
    ///
    /// The PNG is wider than the tactical map, so stretching it to `MAP_SIZE`
    /// bends floor lanes and perspective cues. Cover scaling keeps the source
    /// aspect ratio and lets the camera crop the harmless outer edge.
    fn environment_sprite_size() -> Vec2 {
        let (width, height) = TextureAsset::ReactorSector.spec().pixel_size;
        let source_ratio = width as f32 / height as f32;
        let map_ratio = MAP_SIZE.x / MAP_SIZE.y;
        if source_ratio >= map_ratio {
            Vec2::new(MAP_SIZE.y * source_ratio, MAP_SIZE.y)
        } else {
            Vec2::new(MAP_SIZE.x, MAP_SIZE.x / source_ratio)
        }
    }

    /// Static command-card text prevents per-frame row/vector allocation.
    /// When a unit is selected, the six slots become a StarCraft-style
    /// contextual card instead of continuing to advertise fabricator
    /// production commands that cannot apply to that selection.
    fn command_card_label(&self, index: usize) -> &'static str {
        if self.selected_resource_node.is_some() {
            return match index {
                0 => "G  ASSIGN SURVEYOR",
                1 => "R  FOCUS NODE",
                _ => "",
            };
        }
        match self.selected_structure {
            Some(StructureKind::Relay(_)) => match index {
                0 => "C  GRID PULSE  35",
                1 => "ENGINEER  RESTORE",
                2 => "ONLINE  +SALVAGE",
                _ => "",
            },
            Some(StructureKind::Reactor) => match index {
                0 => "C  CRAFT CORE  90",
                1 => "CORE  +8% DAMAGE",
                2 => "REQUIRES 3 RELAYS",
                _ => "",
            },
            Some(StructureKind::Fabricator) => match index {
                4 if !self.simulation.production.items().is_empty() => "X  CANCEL NEXT",
                0..=4 => COMMAND_CARD_LABELS[index],
                5 if self.placing_beacon => "B  CANCEL BEACON",
                5 => "B BEACON 50",
                _ => "",
            },
            None => match self.selected_single_unit_kind() {
                Some(UnitKind::Warden) => match index {
                    0 => "Y  SURGE  +35%",
                    1 => "A  ATTACK-MOVE",
                    2 => "P  PATROL",
                    3 => "H  HOLD",
                    4 => "U  FOLLOW",
                    5 => "T  STOP",
                    _ => "",
                },
                Some(UnitKind::Engineer) => match index {
                    0 => "Y  EMERGENCY REPAIR",
                    1 => "B  FIELD BEACON",
                    2 => "K  AWAKEN CONSOLE",
                    3 => "H  HOLD",
                    4 => "U  FOLLOW",
                    5 => "T  STOP",
                    _ => "",
                },
                Some(UnitKind::Surveyor) => match index {
                    0 => "Y  SCAN PULSE",
                    1 => "G  HARVEST NODE",
                    2 => "A  ATTACK-MOVE",
                    3 => "P  PATROL",
                    4 => "H  HOLD",
                    5 => "T  STOP",
                    _ => "",
                },
                _ if self.selected_squad_active() => match index {
                    0 => "A  ATTACK-MOVE",
                    1 => "P  PATROL",
                    2 => "H  HOLD",
                    3 => "U  FOLLOW",
                    4 => "T  STOP",
                    5 => "B  FIELD BEACON",
                    _ => "",
                },
                _ => match index {
                    0..=4 => COMMAND_CARD_LABELS[index],
                    5 if self.placing_beacon => "B  CANCEL BEACON",
                    5 => "B BEACON 50",
                    _ => "",
                },
            },
        }
    }

    /// Adds a short prerequisite to structure action rows without changing
    /// the simulation command or its transient error copy. Keeping the
    /// strings compact is important because the bitmap command rows have no
    /// clipping layer and must fit beside the minimap on browser viewports.
    fn command_card_display(&self, index: usize) -> String {
        match self.selected_structure {
            Some(StructureKind::Relay(relay_index)) if index == 0 => {
                let Some(relay) = self.simulation.relays.get(relay_index) else {
                    return "C PULSE // UNAVAILABLE".to_owned();
                };
                if !relay.active {
                    "C PULSE // ENGINEER".to_owned()
                } else if self.simulation.resources.amount() < 35 {
                    "C PULSE // NEED 35".to_owned()
                } else {
                    self.command_card_label(index).to_owned()
                }
            }
            Some(StructureKind::Reactor) if index == 0 => {
                if !self.simulation.relays.iter().all(|relay| relay.active) {
                    "C CORE // RESTORE RELAYS".to_owned()
                } else if self.lumen_cores >= 3 {
                    "C CORE // CAPACITY 3/3".to_owned()
                } else if self.simulation.resources.amount() < 90 {
                    "C CORE // NEED 90".to_owned()
                } else {
                    self.command_card_label(index).to_owned()
                }
            }
            Some(StructureKind::Fabricator) if index <= 2 => self.fabricator_build_copy(index),
            Some(StructureKind::Fabricator) if index == 5 && !self.placing_beacon => {
                format!(
                    "{}  {}",
                    self.command_card_label(index),
                    self.fabricator_module_copy()
                )
            }
            Some(StructureKind::Fabricator)
                if index == 4 && !self.simulation.production.items().is_empty() =>
            {
                if self.simulation.power.is_powered(FABRICATOR_NODE) {
                    format!("X  CANCEL NEXT // {}% REFUND", QUEUE_CANCEL_REFUND_PERCENT)
                } else {
                    "X  CANCEL // OFFLINE".to_owned()
                }
            }
            _ => self.command_card_label(index).to_owned(),
        }
    }

    fn command_card_available(&self, index: usize) -> bool {
        match self.selected_structure {
            Some(StructureKind::Relay(relay_index)) if index == 0 => self
                .simulation
                .relays
                .get(relay_index)
                .is_some_and(|relay| relay.active && self.simulation.resources.amount() >= 35),
            Some(StructureKind::Reactor) if index == 0 => {
                self.simulation.relays.iter().all(|relay| relay.active)
                    && self.lumen_cores < 3
                    && self.simulation.resources.amount() >= 90
            }
            Some(StructureKind::Fabricator) if index <= 2 => Self::fabricator_kind_for_card(index)
                .is_some_and(|kind| self.fabricator_build_gate(kind) == FabricatorBuildGate::Ready),
            Some(StructureKind::Fabricator) if index == 4 => {
                !self.simulation.production.items().is_empty()
                    && self.simulation.power.is_powered(FABRICATOR_NODE)
            }
            _ => true,
        }
    }

    fn command_card_key(&self, index: usize) -> Option<KeyCode> {
        if self.selected_resource_node.is_some() {
            return [KeyCode::KeyG, KeyCode::KeyR].get(index).copied();
        }
        match self.selected_structure {
            Some(StructureKind::Relay(_) | StructureKind::Reactor) => {
                (index == 0).then_some(KeyCode::KeyC)
            }
            Some(StructureKind::Fabricator)
                if index == 4 && !self.simulation.production.items().is_empty() =>
            {
                Some(KeyCode::KeyX)
            }
            Some(StructureKind::Fabricator) => COMMAND_CARD_KEYS.get(index).copied(),
            None => match self.selected_single_unit_kind() {
                Some(UnitKind::Warden) => [
                    KeyCode::KeyY,
                    KeyCode::KeyA,
                    KeyCode::KeyP,
                    KeyCode::KeyH,
                    KeyCode::KeyU,
                    KeyCode::KeyT,
                ]
                .get(index)
                .copied(),
                Some(UnitKind::Engineer) => [
                    KeyCode::KeyY,
                    KeyCode::KeyB,
                    KeyCode::KeyK,
                    KeyCode::KeyH,
                    KeyCode::KeyU,
                    KeyCode::KeyT,
                ]
                .get(index)
                .copied(),
                Some(UnitKind::Surveyor) => [
                    KeyCode::KeyY,
                    KeyCode::KeyG,
                    KeyCode::KeyA,
                    KeyCode::KeyP,
                    KeyCode::KeyH,
                    KeyCode::KeyT,
                ]
                .get(index)
                .copied(),
                _ if self.selected_squad_active() => [
                    KeyCode::KeyA,
                    KeyCode::KeyP,
                    KeyCode::KeyH,
                    KeyCode::KeyU,
                    KeyCode::KeyT,
                    KeyCode::KeyB,
                ]
                .get(index)
                .copied(),
                _ => COMMAND_CARD_KEYS.get(index).copied(),
            },
        }
    }

    fn command_row_for_key(&self, key: KeyCode) -> Option<usize> {
        (0..COMMAND_CARD_KEYS.len())
            .find_map(|index| (self.command_card_key(index) == Some(key)).then_some(index))
    }

    /// The Fabricator's final row intentionally contains two compact actions.
    /// Keyboard input stays explicit (`B`/`D`), while pointer input resolves
    /// the left or right half of that row to the matching action.
    fn command_card_key_at(
        &self,
        index: usize,
        slot: usize,
        point: Vec2,
        card_text: Vec2,
        scale: f32,
    ) -> Option<KeyCode> {
        if matches!(self.selected_structure, Some(StructureKind::Fabricator))
            && index == 5
            && !self.placing_beacon
        {
            let rect = Self::command_card_row_rect(card_text, slot, scale);
            return Some(if point.x >= rect.center().x {
                KeyCode::KeyD
            } else {
                KeyCode::KeyB
            });
        }
        self.command_card_key(index)
    }

    fn activate_selected_ability(&mut self) {
        let Some(id) = self.selected_unit_id() else {
            self.status = Some(("SELECT A LANTERN UNIT FIRST".to_owned(), 2.0));
            return;
        };
        match self.simulation.activate_ability(id) {
            Ok(ability) => {
                self.status = Some((
                    format!("{} // {} ONLINE", ability.speaker(), ability.label()),
                    3.0,
                ));
            }
            Err(AbilityError::Cooldown) => {
                let remaining = self.simulation.ability_cooldown(id).ceil() as u32;
                self.status = Some((format!("ABILITY RECHARGING // {remaining}s"), 2.0));
            }
            Err(AbilityError::NoTarget) => {
                self.status = Some((
                    "EMERGENCY REPAIR // NO DAMAGED ASSET IN RANGE".to_owned(),
                    2.5,
                ));
            }
            Err(AbilityError::NotAvailable) => {
                self.status = Some(("ABILITY UNAVAILABLE FOR THIS SELECTION".to_owned(), 2.0));
            }
        }
    }

    fn arm_attack_move(&mut self) {
        self.attack_move_mode = true;
        self.patrol_mode = false;
        self.follow_mode = false;
        self.status = Some((
            "ATTACK-MOVE READY — RIGHT CLICK DESTINATION".to_owned(),
            3.0,
        ));
    }

    fn arm_patrol(&mut self) {
        self.patrol_mode = true;
        self.attack_move_mode = false;
        self.follow_mode = false;
        self.status = Some(("PATROL READY — RIGHT CLICK WAYPOINT".to_owned(), 3.0));
    }

    fn arm_follow(&mut self) {
        self.follow_mode = true;
        self.attack_move_mode = false;
        self.patrol_mode = false;
        self.status = Some(("FOLLOW READY — RIGHT CLICK A LANTERN UNIT".to_owned(), 3.0));
    }

    fn assign_nearest_harvest_order(&mut self) {
        let Some(position) = self.selected_unit_id().and_then(|id| {
            (self.simulation.kinds.get(&id) == Some(&UnitKind::Surveyor))
                .then(|| self.simulation.world.unit(id).map(|unit| unit.position))
                .flatten()
        }) else {
            self.status = Some(("SELECT A SURVEYOR TO HARVEST".to_owned(), 2.5));
            return;
        };
        let Some((node, _)) = self
            .salvage_nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.remaining > 0)
            .map(|(index, node)| (index, node.position.distance(position)))
            .min_by(|left, right| left.1.total_cmp(&right.1))
        else {
            self.status = Some(("ALL RESOURCE NODES EXHAUSTED".to_owned(), 2.5));
            return;
        };
        let assigned = self.assign_harvest_order(node);
        self.status = Some((
            format!(
                "{assigned} SURVEYOR // {} ROUTE SET",
                match self.salvage_nodes[node].kind {
                    ResourceKind::Salvage => "SALVAGE",
                    ResourceKind::Flux => "FLUX",
                }
            ),
            2.5,
        ));
    }

    /// Returns living Surveyors that do not currently own a harvest route.
    ///
    /// A rallying Surveyor is inserted into `harvest_jobs` before it reaches
    /// the node, so this intentionally treats the whole route as assigned.
    /// That makes the idle count agree with the worker's actual next job,
    /// rather than flickering while the unit is travelling.
    fn idle_surveyor_ids(&self) -> Vec<UnitId> {
        self.simulation
            .world
            .units()
            .iter()
            .filter(|unit| {
                unit.faction == PLAYER
                    && unit.alive()
                    && self.simulation.kinds.get(&unit.id) == Some(&UnitKind::Surveyor)
                    && !self.harvest_jobs.contains_key(&unit.id)
            })
            .map(|unit| unit.id)
            .collect()
    }

    /// Exception-only worker telemetry. The full worker roster remains in
    /// the selection card/resource card; this chip appears only when an idle
    /// Surveyor needs attention, keeping the playfield quiet in the normal
    /// harvesting loop.
    fn idle_surveyor_hud_copy(&self) -> Option<String> {
        let idle = self.idle_surveyor_ids().len();
        (idle > 0).then(|| {
            if idle == 1 {
                "IDLE SURVEYOR 1 // I FOCUS".to_owned()
            } else {
                format!("IDLE S {idle} // I FOCUS")
            }
        })
    }

    /// Select and frame the first idle Surveyor in stable world order. The
    /// command deliberately does not assign a route: the follow-up `G`
    /// action still lets the player choose the nearest live node.
    fn focus_idle_surveyor(&mut self, ctx: &mut FrameCtx<'_>) {
        let Some(id) = self.idle_surveyor_ids().into_iter().next() else {
            self.status = Some(("NO IDLE SURVEYOR".to_owned(), 1.8));
            return;
        };
        let Some(position) = self.simulation.world.unit(id).map(|unit| unit.position) else {
            return;
        };
        self.selected_structure = None;
        self.selected_resource_node = None;
        self.reset_command_card_page();
        self.simulation.world.select_point(position, PLAYER, false);
        ctx.renderer.camera.position = position;
        ctx.renderer
            .camera
            .clamp_to_bounds(Aabb::from_center_size(Vec2::ZERO, MAP_SIZE));
        self.target_feedback = Some((position, "IDLE SURVEYOR".to_owned(), 2.5));
        self.status = Some(("IDLE SURVEYOR FOCUSED // G HARVEST".to_owned(), 2.5));
    }

    fn assign_selected_resource_node(&mut self) {
        let Some(node) = self.selected_resource_node else {
            self.status = Some(("SELECT A RESOURCE NODE FIRST".to_owned(), 2.0));
            return;
        };
        let Some((_, remaining, max_workers, node_kind)) = self
            .salvage_nodes
            .get(node)
            .map(|node| (node.position, node.remaining, node.max_workers, node.kind))
        else {
            self.status = Some(("RESOURCE NODE UNAVAILABLE".to_owned(), 2.0));
            return;
        };
        if remaining == 0 {
            self.status = Some(("RESOURCE NODE DRY".to_owned(), 2.0));
            return;
        }
        let capacity = max_workers as usize;
        let occupied = self.workers_at_node(node);
        let slots = capacity.saturating_sub(occupied);
        if slots == 0 {
            self.status = Some(("RESOURCE NODE SATURATED".to_owned(), 2.0));
            return;
        }
        let candidates: Vec<UnitId> = self.nearest_idle_surveyors_to_node(node);
        let assigned =
            self.assign_surveyors_to_node(node, &candidates[..candidates.len().min(slots)]);
        let kind = match node_kind {
            ResourceKind::Salvage => "SALVAGE",
            ResourceKind::Flux => "FLUX",
        };
        self.status = Some((
            if assigned > 0 {
                format!("{assigned} SURVEYOR // {kind} ROUTE SET")
            } else {
                "NO AVAILABLE SURVEYOR".to_owned()
            },
            2.5,
        ));
    }

    fn focus_resource_node(&mut self, ctx: &mut FrameCtx<'_>) {
        let Some(node) = self.selected_resource_node else {
            self.status = Some(("SELECT A RESOURCE NODE FIRST".to_owned(), 2.0));
            return;
        };
        let Some(position) = self.salvage_nodes.get(node).map(|node| node.position) else {
            self.status = Some(("RESOURCE NODE UNAVAILABLE".to_owned(), 2.0));
            return;
        };
        ctx.renderer.camera.position = position;
        ctx.renderer
            .camera
            .clamp_to_bounds(Aabb::from_center_size(Vec2::ZERO, MAP_SIZE));
        self.order_marker = Some((position, 1.5));
        self.target_feedback = Some((position, format!("RESOURCE NODE {}", node + 1), 3.0));
        self.status = Some((format!("RESOURCE FOCUS // NODE {}", node + 1), 2.0));
    }

    fn awaken_lumen_console(&mut self) {
        let Some(console) = self.mission.lumen_console else {
            self.status = Some(("NO LUMEN CONSOLE IN THIS MISSION".to_owned(), 2.0));
            return;
        };
        if self.save_data.campaign.has_decision(LUMEN_AWAKENED) {
            self.status = Some(("LUMEN CONSOLE ALREADY AWAKE".to_owned(), 2.0));
            return;
        }
        if !self.selected_engineer_near(console) {
            self.status = Some(("ENGINEER MUST BE IN RANGE OF THE CONSOLE".to_owned(), 2.5));
            return;
        }
        self.save_data.campaign.record_decision(LUMEN_AWAKENED);
        let _ = self
            .save_store
            .save(&save::envelope(self.save_data.clone()));
        self.last_transmission = Some(console);
        self.status = Some(("LUMEN CONSOLE AWAKENED".to_owned(), 4.0));
    }

    fn activate_structure_command(&mut self, structure: StructureKind) {
        match structure {
            StructureKind::Fabricator => {}
            StructureKind::Relay(index) => {
                let Some(relay) = self.simulation.relays.get(index) else {
                    return;
                };
                if !relay.active {
                    self.status = Some(("RELAY OFFLINE — ENGINEER REQUIRED".to_owned(), 2.5));
                    return;
                }
                if !self.simulation.resources.spend(35) {
                    self.status = Some(("GRID PULSE REQUIRES 35 SALVAGE".to_owned(), 2.5));
                    return;
                }
                let position = relay.position;
                let mut restored = 0;
                for unit in self.simulation.world.units_mut().iter_mut().filter(|unit| {
                    unit.faction == PLAYER
                        && unit.alive()
                        && unit.position.distance(position) < 420.0
                }) {
                    let before = unit.health;
                    unit.health = (unit.health + 35.0).min(unit.max_health);
                    restored += u32::from(unit.health > before);
                }
                self.status = Some((
                    format!("RELAY {} GRID PULSE // {restored} RESTORED", index + 1),
                    3.0,
                ));
            }
            StructureKind::Reactor => {
                if !self.simulation.relays.iter().all(|relay| relay.active) {
                    self.status = Some(("REACTOR LOCKED — RESTORE FULL LATTICE".to_owned(), 3.0));
                    return;
                }
                if self.lumen_cores >= 3 {
                    self.status = Some(("LUMEN CORE CAPACITY 3/3".to_owned(), 2.5));
                    return;
                }
                if !self.simulation.activate_lumen_core() {
                    self.status = Some(("LUMEN CORE REQUIRES 90 SALVAGE".to_owned(), 2.5));
                    return;
                }
                self.lumen_cores += 1;
                self.simulation
                    .set_combat_scales(1.0 + self.lumen_cores as f32 * 0.08, 1.0);
                self.status = Some((format!("LUMEN CORE CRAFTED // {}/3", self.lumen_cores), 3.5));
            }
        }
    }

    fn command_card_row_rect(card_text: Vec2, slot: usize, scale: f32) -> Aabb {
        // One compact column keeps page transitions obvious and avoids moving
        // the anchor point when additional pages are shown.
        let row = slot % COMMAND_CARD_COMPACT_ROWS;
        let center = card_text + Vec2::new(130.0, -38.0 - row as f32 * 30.0) * scale;
        Aabb::from_center_size(center, Vec2::new(250.0, 26.0) * scale)
    }

    fn apply_command_action(&mut self, key: KeyCode) {
        let Some(row) = self.command_row_for_key(key) else {
            return;
        };
        if !self.command_card_available(row) {
            return;
        }

        if self.selected_resource_node.is_some() {
            match key {
                KeyCode::KeyG => self.assign_selected_resource_node(),
                KeyCode::KeyR => {}
                _ => {}
            }
            return;
        }

        if let Some(structure) = self.selected_structure {
            match structure {
                StructureKind::Relay(_) | StructureKind::Reactor => {
                    if key == KeyCode::KeyC {
                        self.activate_structure_command(structure);
                    }
                }
                StructureKind::Fabricator => match key {
                    KeyCode::KeyQ => self.queue_unit(UnitKind::Warden),
                    KeyCode::KeyE => self.queue_unit(UnitKind::Engineer),
                    KeyCode::KeyF => self.queue_unit(UnitKind::Surveyor),
                    KeyCode::KeyX => self.cancel_queued_unit(0),
                    KeyCode::KeyB => {
                        self.placing_beacon = !self.placing_beacon;
                        self.status = Some((
                            if self.placing_beacon {
                                "BEACON PLACEMENT — LEFT CLICK / ESC CANCEL"
                            } else {
                                "BEACON PLACEMENT CANCELLED"
                            }
                            .to_owned(),
                            3.0,
                        ));
                    }
                    _ => {}
                },
            }
            return;
        }

        match key {
            KeyCode::KeyY => {
                if self.selected_single_unit_kind().is_some() {
                    self.activate_selected_ability();
                }
            }
            KeyCode::KeyA => self.arm_attack_move(),
            KeyCode::KeyP => self.arm_patrol(),
            KeyCode::KeyU => self.arm_follow(),
            KeyCode::KeyH => {
                self.simulation.world.issue_hold();
                self.status = Some(("SQUAD HOLDING POSITION".to_owned(), 2.0));
            }
            KeyCode::KeyT => {
                self.simulation.world.issue_stop();
                self.simulation.player_paths.clear();
                self.status = Some(("SQUAD ORDERS STOPPED".to_owned(), 2.0));
            }
            KeyCode::KeyB => {
                self.placing_beacon = !self.placing_beacon;
                self.status = Some((
                    if self.placing_beacon {
                        "BEACON PLACEMENT — LEFT CLICK / ESC CANCEL"
                    } else {
                        "BEACON PLACEMENT CANCELLED"
                    }
                    .to_owned(),
                    3.0,
                ));
            }
            KeyCode::KeyG => {
                if self.selected_single_unit_kind() == Some(UnitKind::Surveyor) {
                    self.assign_nearest_harvest_order();
                }
            }
            KeyCode::KeyK => {
                if self.selected_single_unit_kind() == Some(UnitKind::Engineer) {
                    self.awaken_lumen_console();
                }
            }
            _ => {}
        }
    }

    fn control_group_action(&mut self, slot: usize, assign: bool, ctx: &mut FrameCtx<'_>) {
        if assign {
            self.simulation.world.assign_control_group(slot);
            self.status = Some((format!("CONTROL GROUP {slot} ASSIGNED"), 2.0));
        } else if self.simulation.world.recall_control_group(slot, PLAYER) {
            let (sum, count) = self
                .simulation
                .world
                .selection()
                .ids()
                .iter()
                .filter_map(|id| self.simulation.world.unit(*id))
                .fold((Vec2::ZERO, 0_u32), |(sum, count), unit| {
                    (sum + unit.position, count + 1)
                });
            if count > 0 {
                ctx.renderer.camera.position = sum / count as f32;
            }
            self.status = Some((format!("CONTROL GROUP {slot}"), 1.5));
        }
    }

    fn control_group_chip_rect(panel: Aabb, slot: usize, scale: f32) -> Aabb {
        let spacing = 46.0 * scale;
        let start_x = panel.center().x - spacing * 2.0;
        let x = start_x + (slot - 1) as f32 * spacing;
        let y = panel.max.y + 26.0 * scale;
        Aabb::from_center_size(Vec2::new(x, y), Vec2::splat(38.0 * scale))
    }

    fn pause_icon_rect(renderer: &Renderer) -> Aabb {
        let scale = Self::hud_scale(renderer);
        let top_right = renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(1.0, 1.0));
        Aabb::from_center_size(
            top_right + Vec2::new(-44.0, -44.0) * scale,
            Vec2::splat(48.0 * scale),
        )
    }

    /// World-space anchor for the command card's text/rows. The single
    /// source of truth for both rendering (`on_update`) and click
    /// hit-testing (`handle_command_keys`) so they can't drift apart.
    fn command_card_text_origin(renderer: &Renderer) -> Vec2 {
        let scale = Self::hud_scale(renderer);
        let bottom_right = renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(1.0, 0.0));
        bottom_right + Vec2::new(-525.0, 258.0) * scale
    }

    fn unit_card_origin(renderer: &Renderer) -> Vec2 {
        let scale = Self::hud_scale(renderer);
        renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(0.0, 0.0))
            + Vec2::new(300.0, 34.0) * scale
    }

    fn selection_chip_rect(renderer: &Renderer, index: usize) -> Aabb {
        let scale = Self::hud_scale(renderer);
        Aabb::from_center_size(
            Self::unit_card_origin(renderer) + Vec2::new(144.0 + index as f32 * 48.0, 93.0) * scale,
            Vec2::splat(38.0 * scale),
        )
    }

    fn handle_command_keys(&mut self, ctx: &mut FrameCtx<'_>) {
        if ctx.input.key_pressed(KeyCode::Tab) {
            self.minimal_hud = !self.minimal_hud;
            self.status = Some((
                if self.minimal_hud {
                    "HUD MODE // MINIMAL".to_owned()
                } else {
                    "HUD MODE // NORMAL".to_owned()
                },
                1.6,
            ));
        }
        if ctx.input.key_pressed(KeyCode::F1) {
            self.controls_hint_remaining = 5.0;
        }
        if ctx.input.key_pressed(KeyCode::KeyM) {
            self.command_card_compact = !self.command_card_compact;
            self.reset_command_card_page();
            self.status = Some((
                format!(
                    "CMD CARD {} // KEY M TO TOGGLE",
                    if self.command_card_compact {
                        "COMPACT"
                    } else {
                        "FULL"
                    }
                ),
                1.6,
            ));
        }
        if ctx.input.key_pressed(KeyCode::KeyI) {
            self.focus_idle_surveyor(ctx);
        }
        if ctx.input.key_pressed(KeyCode::KeyR) {
            if self.selected_resource_node.is_some() {
                if self.command_row_for_key(KeyCode::KeyR).is_some() {
                    self.focus_resource_node(ctx);
                }
            } else {
                self.focus_next_objective(ctx);
            }
        }
        if ctx.input.key_pressed(KeyCode::Space) {
            self.focus_last_transmission(ctx);
        }
        if self.command_card_should_paginate(ctx.renderer)
            && ctx.input.key_pressed(KeyCode::ArrowUp)
        {
            self.prev_command_card_page(ctx.renderer);
        }
        if self.command_card_should_paginate(ctx.renderer)
            && ctx.input.key_pressed(KeyCode::ArrowDown)
        {
            self.next_command_card_page(ctx.renderer);
        }
        for key in [
            KeyCode::KeyA,
            KeyCode::KeyP,
            KeyCode::KeyU,
            KeyCode::KeyY,
            KeyCode::KeyG,
            KeyCode::KeyK,
            KeyCode::KeyH,
            KeyCode::KeyT,
            KeyCode::KeyB,
            KeyCode::KeyQ,
            KeyCode::KeyE,
            KeyCode::KeyF,
            KeyCode::KeyC,
            KeyCode::KeyX,
        ] {
            if ctx.input.key_pressed(key) {
                if let Some(row) = self.command_row_for_key(key) {
                    let visible_rows = self.visible_command_card_rows_for_display(ctx.renderer);
                    if visible_rows.contains(&row) {
                        self.apply_command_action(key);
                    }
                }
            }
        }
        if ctx.input.key_pressed(KeyCode::KeyD)
            && matches!(self.selected_structure, Some(StructureKind::Fabricator))
            && self
                .visible_command_card_rows_for_display(ctx.renderer)
                .contains(&5)
        {
            self.upgrade_supply_module();
        }

        for slot in 1..=5 {
            let key = match slot {
                1 => KeyCode::Digit1,
                2 => KeyCode::Digit2,
                3 => KeyCode::Digit3,
                4 => KeyCode::Digit4,
                _ => KeyCode::Digit5,
            };
            if ctx.input.key_pressed(key) {
                self.control_group_action(slot, ctx.input.control_down(), ctx);
            }
        }

        if ctx.input.mouse_pressed(MouseButton::Left) {
            let mouse_world = ctx
                .renderer
                .camera
                .screen_to_world(ctx.input.mouse_position);
            let scale = Self::hud_scale(ctx.renderer);
            if self.command_card_visible() {
                let card_text = Self::command_card_text_origin(ctx.renderer);
                let visible_rows = self.visible_command_card_rows_for_display(ctx.renderer);
                for (slot, &index) in visible_rows.iter().enumerate() {
                    if Self::command_card_row_rect(card_text, slot, scale)
                        .contains_point(mouse_world)
                    {
                        if !self.command_card_available(index) {
                            return;
                        }
                        if let Some(key) =
                            self.command_card_key_at(index, slot, mouse_world, card_text, scale)
                        {
                            if key == KeyCode::KeyD
                                && matches!(
                                    self.selected_structure,
                                    Some(StructureKind::Fabricator)
                                )
                            {
                                self.upgrade_supply_module();
                            } else {
                                self.apply_command_action(key);
                            }
                        }
                        return;
                    }
                }
            }
            let panel = self.minimap_transform(ctx.renderer).panel;
            for slot in 1..=5 {
                if Self::control_group_chip_rect(panel, slot, scale).contains_point(mouse_world) {
                    self.control_group_action(slot, ctx.input.control_down(), ctx);
                    return;
                }
            }
            let selected_ids = self.simulation.world.selection().ids().to_vec();
            if selected_ids.len() > 1 {
                for (index, id) in selected_ids.iter().take(5).enumerate() {
                    if !Self::selection_chip_rect(ctx.renderer, index).contains_point(mouse_world) {
                        continue;
                    }
                    let Some(position) = self
                        .simulation
                        .world
                        .unit(*id)
                        .filter(|unit| unit.faction == PLAYER && unit.alive())
                        .map(|unit| unit.position)
                    else {
                        continue;
                    };
                    self.selected_structure = None;
                    self.simulation.world.select_point(position, PLAYER, false);
                    self.reset_command_card_page();
                    self.status = Some((
                        "SPECIALIST SELECTED // COMMAND CARD UPDATED".to_owned(),
                        1.8,
                    ));
                    return;
                }
            }
        }
    }

    fn update_status_timer(&mut self, dt: f32) {
        self.controls_hint_remaining = (self.controls_hint_remaining - dt.max(0.0)).max(0.0);
        if let Some((_, remaining)) = self.status.as_mut() {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.status = None;
            }
        }
        if let Some((_, _, remaining)) = self.target_feedback.as_mut() {
            *remaining -= dt.max(0.0);
            if *remaining <= 0.0 {
                self.target_feedback = None;
            }
        }
    }

    fn process_simulation_events(&mut self, ctx: &mut FrameCtx<'_>) {
        while let Some(event) = self.simulation.pop_pending_event() {
            match event.kind {
                SimulationEventKind::RelayActivated { index } => {
                    ctx.audio.win_note();
                    let position = self
                        .simulation
                        .relays
                        .get(index)
                        .map(|relay| relay.position);
                    self.queue_radio_line(
                        "IVO ROOK",
                        "Relay handshake confirmed. The fabricator is breathing again.",
                        position,
                    );
                }
                SimulationEventKind::UnitDeployed { unit_id, kind } => {
                    self.animation_players
                        .insert(UnitId(unit_id), AnimationPlayer::default());
                    self.status = Some((format!("{} DEPLOYED", kind.label()), 3.0));
                    if kind == UnitKind::Surveyor {
                        // A resource rally is a production affordance, not a
                        // second worker-assignment command. The helper keeps
                        // ordinary rally points as normal move destinations.
                        self.apply_surveyor_rally(UnitId(unit_id));
                    }
                }
                SimulationEventKind::UnitSpawned { unit_id, .. } => {
                    self.animation_players
                        .insert(UnitId(unit_id), AnimationPlayer::default());
                }
                SimulationEventKind::AttackLanded { attacker, target } => {
                    self.attack_flash.insert(UnitId(attacker), 0.08);
                    self.damage_flash.insert(UnitId(target), 0.34);
                }
                SimulationEventKind::TargetAcquired { attacker, target } => {
                    let attacker_id = UnitId(attacker);
                    if self
                        .simulation
                        .world
                        .unit(attacker_id)
                        .is_some_and(|unit| unit.faction == PLAYER)
                    {
                        if let Some(target_unit) = self.simulation.world.unit(UnitId(target)) {
                            let label = self
                                .simulation
                                .kinds
                                .get(&UnitId(target))
                                .map(|kind| kind.label())
                                .unwrap_or("CONTACT");
                            self.target_feedback = Some((
                                target_unit.position,
                                format!("TARGET LOCK // {label}"),
                                1.8,
                            ));
                        }
                    }
                }
                SimulationEventKind::TargetLost { attacker } => {
                    if self
                        .simulation
                        .world
                        .unit(UnitId(attacker))
                        .is_some_and(|unit| unit.faction == PLAYER)
                    {
                        self.status = Some(("TARGET LOST // REACQUIRE".to_owned(), 1.2));
                    }
                }
                SimulationEventKind::AttackTelegraph {
                    attacker,
                    target,
                    windup_seconds,
                } => {
                    let attacker_kind = self.simulation.kinds.get(&UnitId(attacker)).copied();
                    let attacker_is_choir = self
                        .simulation
                        .world
                        .unit(UnitId(attacker))
                        .is_some_and(|unit| unit.faction == CHOIR);
                    if attacker_is_choir {
                        if let Some(target_unit) = self.simulation.world.unit(UnitId(target)) {
                            let label = attacker_kind.map(UnitKind::label).unwrap_or("CONTACT");
                            self.target_feedback = Some((
                                target_unit.position,
                                format!("INCOMING // {label}  {:.1}s", windup_seconds),
                                1.8,
                            ));
                        }
                    }
                }
                SimulationEventKind::DamageApplied { target } => {
                    self.damage_flash.insert(UnitId(target), 0.34);
                }
                SimulationEventKind::UnitRepaired { engineer, target } => {
                    self.repair_flash
                        .insert(UnitId(engineer), (UnitId(target), 0.12));
                }
                SimulationEventKind::StructureRepaired { .. } => {}
                SimulationEventKind::UnitDestroyed { unit_id, kind } => {
                    let unit_id = UnitId(unit_id);
                    self.down_units.insert(unit_id, 0.0);
                    self.damage_flash.remove(&unit_id);
                    let player_loss = self
                        .simulation
                        .world
                        .unit(unit_id)
                        .is_some_and(|unit| unit.faction == PLAYER);
                    if player_loss {
                        let position = self
                            .simulation
                            .world
                            .unit(unit_id)
                            .map(|unit| unit.position);
                        self.queue_urgent_radio_line(
                            "MARA VEY",
                            match kind {
                                UnitKind::Warden => {
                                    "Warden down. Re-form the line and keep the relay lit."
                                }
                                UnitKind::Engineer => {
                                    "We lost our hands. Protect the next Engineer."
                                }
                                UnitKind::Surveyor => {
                                    "Surveyor offline. Salvage routes are exposed."
                                }
                                _ => "Lantern contact lost. Pull back and stabilize.",
                            },
                            position,
                        );
                    }
                }
                SimulationEventKind::BossReinforced => {
                    self.status = Some(("CANTICLE CALLS REINFORCEMENTS".to_owned(), 4.0));
                    ctx.audio.hurt();
                }
                SimulationEventKind::EnemyRaidSpawned { unit_id, kind } => {
                    self.status = Some((format!("CHOIR RAID // {} INBOUND", kind.label()), 4.0));
                    ctx.audio.hurt();
                    let position = self
                        .simulation
                        .world
                        .unit(UnitId(unit_id))
                        .map(|unit| unit.position);
                    self.queue_urgent_radio_line(
                        "PREFECT VALE",
                        "The Choir is spending against us. Break the raid before it reaches the lattice.",
                        position,
                    );
                }
                SimulationEventKind::EnemyRaidTelegraph {
                    number,
                    kind,
                    spawn_x,
                    spawn_y,
                    seconds_remaining,
                    ..
                } => {
                    self.target_feedback = Some((
                        Vec2::new(spawn_x, spawn_y),
                        format!(
                            "RAID {number} // {} IN {:02}s",
                            kind.label(),
                            seconds_remaining.ceil() as u32
                        ),
                        2.4,
                    ));
                }
                SimulationEventKind::UnitRetreating { unit_id, kind } => {
                    if let Some(unit) = self.simulation.world.unit(UnitId(unit_id)) {
                        self.target_feedback =
                            Some((unit.position, format!("{} WITHDRAWING", kind.label()), 1.6));
                    }
                }
                SimulationEventKind::UnitRecovered { unit_id, kind } => {
                    if let Some(unit) = self.simulation.world.unit(UnitId(unit_id)) {
                        self.target_feedback =
                            Some((unit.position, format!("{} BACK ONLINE", kind.label()), 1.6));
                    }
                }
                SimulationEventKind::AbilityActivated { unit_id, ability } => {
                    let position = self
                        .simulation
                        .world
                        .unit(UnitId(unit_id))
                        .map(|unit| unit.position);
                    let text = match ability {
                        SpecialAbility::CommandSurge => {
                            "Command surge live. Lanterns, advance on my mark."
                        }
                        SpecialAbility::EmergencyRepair => {
                            "Emergency repair is running. Hold the perimeter."
                        }
                        SpecialAbility::ScanPulse => {
                            "Scan pulse live. I am painting contacts on your map."
                        }
                    };
                    self.queue_radio_line(ability.speaker(), text, position);
                }
                SimulationEventKind::StructureBuildQueued { structure } => {
                    self.status = Some((format!("{structure} CONSTRUCTION QUEUED"), 3.0));
                }
                SimulationEventKind::StructureBuildCompleted { structure } => {
                    self.status = Some((format!("{structure} ONLINE // SUPPLY EXPANDED"), 3.5));
                    self.queue_radio_line(
                        "IVO ROOK",
                        "Module seated. The Lantern line can take another wave.",
                        Some(self.fabricator_position),
                    );
                }
                SimulationEventKind::MissionVictory => self.victory = true,
                SimulationEventKind::MissionDefeat => self.defeat = true,
                SimulationEventKind::CommandAccepted { .. }
                | SimulationEventKind::ResourcesCredited { .. }
                | SimulationEventKind::ResourcesDelivered { .. }
                | SimulationEventKind::UnitQueued { .. } => {}
            }
        }
    }

    fn queue_radio_line(
        &mut self,
        speaker: &'static str,
        text: &'static str,
        position: Option<Vec2>,
    ) {
        self.queue_radio_line_with_priority(speaker, text, position, false);
    }

    fn queue_urgent_radio_line(
        &mut self,
        speaker: &'static str,
        text: &'static str,
        position: Option<Vec2>,
    ) {
        self.queue_radio_line_with_priority(speaker, text, position, true);
    }

    fn queue_radio_line_with_priority(
        &mut self,
        speaker: &'static str,
        text: &'static str,
        position: Option<Vec2>,
        urgent: bool,
    ) {
        if let Some(position) = position {
            self.last_transmission = Some(position);
            self.target_feedback = Some((position, format!("COMMS // {speaker}"), 3.5));
        }
        if self.radio_message.is_none() {
            self.radio_message = Some((speaker, text, 6.0));
            self.radio_pop_in = 1.0;
        } else if urgent {
            self.radio_priority_queue
                .push_back((speaker, text, position));
        } else {
            self.radio_queue.push_back((speaker, text, position));
        }
    }

    fn persist_victory(&mut self) {
        if self.victory_saved {
            return;
        }
        self.save_data.runs_completed = self.save_data.runs_completed.saturating_add(1);
        self.save_data.campaign.complete_mission(
            self.mission.id,
            self.mission.unlock_next,
            self.mission.reward_lumen,
        );
        if let Some(decision) = self.mission.unlock_decision {
            self.save_data.campaign.record_decision(decision);
        }
        let unlock_next = self.mission.unlock_next;
        self.status = Some(
            match self
                .save_store
                .save(&save::envelope(self.save_data.clone()))
            {
                Ok(()) => (
                    format!("CAMPAIGN SAVED — MISSION {unlock_next} UNLOCKED"),
                    8.0,
                ),
                Err(error) => (format!("SAVE FAILED: {error}"), 8.0),
            },
        );
        self.victory_saved = true;
    }

    fn campaign_consequence(&self) -> &'static str {
        match self.mission.unlock_decision {
            Some(MERIDIAN_ALLIED) => "CONSEQUENCE // MERIDIAN ACCORD AVAILABLE",
            Some(LUMEN_CONTACT) => "CONSEQUENCE // LUMEN CONTACT ESTABLISHED",
            Some(LUMEN_AWAKENED) => "CONSEQUENCE // LUMEN AWAKENED",
            Some(_) => "CONSEQUENCE // CAMPAIGN STATE CHANGED",
            None => "CONSEQUENCE // SPECIALIST RECORDS UPDATED",
        }
    }

    /// Mirrors the simulation-owned outcome into presentation flags used by
    /// the end-of-mission overlays and campaign persistence.
    fn evaluate_mission_state(&mut self) {
        self.victory = self.simulation.outcome == MissionOutcome::Victory;
        self.defeat = self.simulation.outcome == MissionOutcome::Defeat;
    }

    fn handle_terminal_input(&mut self, key: KeyCode) {
        match key {
            KeyCode::Space | KeyCode::Enter | KeyCode::NumpadEnter if self.defeat => {
                self.start_mission(self.mission.clone());
            }
            KeyCode::Space | KeyCode::Enter | KeyCode::NumpadEnter if self.victory => {
                self.mission_cursor = missions::all()
                    .iter()
                    .position(|mission| mission.id == self.mission.id)
                    .unwrap_or(self.mission_cursor);
                self.mission_select = true;
                self.briefing = false;
                self.victory = false;
                self.defeat = false;
            }
            KeyCode::Escape if self.victory || self.defeat => {
                self.mission_cursor = missions::all()
                    .iter()
                    .position(|mission| mission.id == self.mission.id)
                    .unwrap_or(self.mission_cursor);
                self.mission_select = true;
                self.briefing = false;
                self.victory = false;
                self.defeat = false;
            }
            _ => {}
        }
    }

    fn selected_engineer_near(&self, position: Vec2) -> bool {
        self.simulation.selected_engineer_near(position)
    }

    /// A high-priority, contextual explanation for the Engineer's relay job.
    /// It deliberately derives from selection + distance, the same conditions
    /// that advance relay progress, so the HUD cannot promise an interaction
    /// the simulation will not perform.
    fn engineer_relay_status(&self) -> Option<String> {
        self.simulation
            .relays
            .iter()
            .enumerate()
            .find_map(|(index, relay)| {
                (!relay.active && self.selected_engineer_near(relay.position)).then(|| {
                    format!(
                        "ENGINEER LINK // RELAY {} — RESTORING {:02}%",
                        index + 1,
                        (relay.progress / 3.0 * 100.0).clamp(0.0, 100.0) as u32
                    )
                })
            })
    }

    fn closest_enemy_at(&self, point: Vec2) -> Option<UnitId> {
        self.simulation
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == CHOIR && unit.alive())
            .filter(|unit| self.fog.state_at(unit.position) == FogState::Visible)
            .filter_map(|unit| {
                let distance = unit.position.distance(point);
                let radius = (unit.radius * ENEMY_CLICK_RADIUS_SCALE).max(62.0);
                (distance <= radius).then_some((unit.id, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    fn structure_at(&self, point: Vec2) -> Option<StructureKind> {
        if let Some(reactor_position) = self.reactor_position {
            if reactor_position.distance(point) <= StructureKind::REACTOR_RADIUS {
                return Some(StructureKind::Reactor);
            }
        }
        if self.fabricator_position.distance(point) <= StructureKind::FABRICATOR_RADIUS {
            return Some(StructureKind::Fabricator);
        }
        self.simulation
            .relays
            .iter()
            .position(|relay| relay.position.distance(point) <= StructureKind::RELAY_RADIUS)
            .map(StructureKind::Relay)
    }

    fn salvage_node_at(&self, point: Vec2) -> Option<usize> {
        self.salvage_nodes
            .iter()
            .position(|node| node.position.distance(point) <= RESOURCE_CLICK_RADIUS)
    }

    fn workers_at_node(&self, node: usize) -> usize {
        self.harvest_jobs
            .values()
            .filter(|job| job.node == node)
            .count()
    }

    /// Finds the next node a persistent Surveyor route can use. The ordering
    /// is explicit so two clients replaying the same mission choose the same
    /// resource pocket when distances tie.
    fn nearest_available_harvest_node(
        &self,
        position: Vec2,
        excluded_node: Option<usize>,
    ) -> Option<usize> {
        self.salvage_nodes
            .iter()
            .enumerate()
            .filter(|(index, node)| {
                node.remaining > 0
                    && Some(*index) != excluded_node
                    && self.workers_at_node(*index) < node.max_workers as usize
            })
            .map(|(index, node)| (index, node.position.distance(position)))
            .min_by(|left, right| {
                left.1
                    .total_cmp(&right.1)
                    .then_with(|| left.0.cmp(&right.0))
            })
            .map(|(index, _)| index)
    }

    fn continue_harvest_route(
        &mut self,
        id: UnitId,
        position: Vec2,
        excluded_node: Option<usize>,
    ) -> bool {
        let Some(node) = self.nearest_available_harvest_node(position, excluded_node) else {
            return false;
        };
        if let Some(job) = self.harvest_jobs.get_mut(&id) {
            job.node = node;
            job.phase = HarvestPhase::ToNode;
        }
        self.simulation
            .issue_unit_move(id, self.salvage_nodes[node].position);
        true
    }

    /// Applies a Fabricator rally to a newly deployed Surveyor when the rally
    /// marker sits on a live resource node. The existing harvest route is the
    /// single source of truth for extraction; this helper only creates its
    /// first `HarvestJob` and leaves ordinary rally movement untouched.
    fn assign_deployed_surveyor_to_rally(&mut self, id: UnitId) -> RallyHarvestOutcome {
        let Some(rally) = self.simulation.rally_point else {
            return RallyHarvestOutcome::NotResource;
        };
        let Some(node) = self.salvage_node_at(rally) else {
            return RallyHarvestOutcome::NotResource;
        };
        if let Some(job) = self.harvest_jobs.get(&id) {
            return RallyHarvestOutcome::AlreadyAssigned(job.node);
        }
        let Some(resource) = self.salvage_nodes.get(node) else {
            return RallyHarvestOutcome::NotResource;
        };
        if resource.remaining == 0 {
            return RallyHarvestOutcome::Dry(node);
        }
        if self.workers_at_node(node) >= resource.max_workers as usize {
            return RallyHarvestOutcome::Saturated(node);
        }
        if self.assign_surveyors_to_node(node, &[id]) == 1 {
            RallyHarvestOutcome::Assigned(node)
        } else {
            // Keep this defensive branch explicit: a future assignment rule
            // can reject the job without accidentally reporting success.
            RallyHarvestOutcome::Saturated(node)
        }
    }

    /// Applies the rally result to the compact tactical status/comms surfaces.
    /// Resource rally feedback is intentionally short-lived; the world marker
    /// and the worker's HarvestJob remain the durable state.
    fn apply_surveyor_rally(&mut self, id: UnitId) -> RallyHarvestOutcome {
        let outcome = self.assign_deployed_surveyor_to_rally(id);
        let node_info = |nodes: &[SalvageNode], node: usize| {
            nodes.get(node).map(|resource| {
                let label = match resource.kind {
                    ResourceKind::Salvage => "SALVAGE",
                    ResourceKind::Flux => "FLUX",
                };
                (label, resource.position)
            })
        };
        match outcome {
            RallyHarvestOutcome::Assigned(node) => {
                if let Some((kind, position)) = node_info(&self.salvage_nodes, node) {
                    self.status = Some((
                        format!("SURVEYOR RALLY // {kind} NODE {} // HARVEST", node + 1),
                        3.0,
                    ));
                    self.queue_radio_line(
                        "SENA QUILL",
                        "Rally accepted. Surveyor is on the extraction route.",
                        Some(position),
                    );
                }
            }
            RallyHarvestOutcome::AlreadyAssigned(node) => {
                self.status = Some((
                    format!("SURVEYOR RALLY // ROUTE PRESERVED // NODE {}", node + 1),
                    2.5,
                ));
            }
            RallyHarvestOutcome::Saturated(node) => {
                if let Some((kind, position)) = node_info(&self.salvage_nodes, node) {
                    self.status = Some((
                        format!("{kind} NODE {} SATURATED // RALLY HELD", node + 1),
                        3.0,
                    ));
                    self.queue_radio_line(
                        "SENA QUILL",
                        "That extraction patch is full. The new Surveyor is holding at rally.",
                        Some(position),
                    );
                }
            }
            RallyHarvestOutcome::Dry(node) => {
                if let Some((kind, position)) = node_info(&self.salvage_nodes, node) {
                    self.status =
                        Some((format!("{kind} NODE {} DRY // RALLY HELD", node + 1), 3.0));
                    self.queue_radio_line(
                        "SENA QUILL",
                        "The rally patch is exhausted. Surveyor is holding at the waypoint.",
                        Some(position),
                    );
                }
            }
            RallyHarvestOutcome::NotResource => {}
        }
        outcome
    }

    fn assign_surveyors_to_node(&mut self, node: usize, surveyors: &[UnitId]) -> usize {
        let Some((position, max_workers)) = self
            .salvage_nodes
            .get(node)
            .map(|node| (node.position, node.max_workers as usize))
        else {
            return 0;
        };
        // Re-assigning a selected Surveyor is explicit and predictable: it
        // leaves its old route before checking the destination's saturation.
        for surveyor in surveyors {
            self.harvest_jobs.remove(surveyor);
        }
        let available = max_workers.saturating_sub(self.workers_at_node(node));
        for surveyor in surveyors.iter().take(available) {
            self.mark_flash.insert(*surveyor, 1.2);
            self.harvest_jobs.insert(
                *surveyor,
                HarvestJob {
                    node,
                    cargo: 0,
                    phase: HarvestPhase::ToNode,
                },
            );
            self.simulation.issue_unit_move(*surveyor, position);
        }
        surveyors.len().min(available)
    }

    fn nearest_idle_surveyors_to_node(&self, node: usize) -> Vec<UnitId> {
        let Some((position, capacity)) = self.salvage_nodes.get(node).map(|salvage_node| {
            let max_workers = salvage_node.max_workers as usize;
            let occupied = self.workers_at_node(node);
            (salvage_node.position, max_workers.saturating_sub(occupied))
        }) else {
            return Vec::new();
        };
        if capacity == 0 {
            return Vec::new();
        }
        let mut candidates: Vec<(UnitId, f32)> = self
            .simulation
            .world
            .units()
            .iter()
            .filter(|unit| {
                unit.faction == PLAYER
                    && unit.alive()
                    && self.simulation.kinds.get(&unit.id) == Some(&UnitKind::Surveyor)
                    && !self.harvest_jobs.contains_key(&unit.id)
            })
            .map(|unit| (unit.id, unit.position.distance(position)))
            .collect();
        candidates.sort_by(|left, right| {
            left.1
                .total_cmp(&right.1)
                .then_with(|| left.0 .0.cmp(&right.0 .0))
        });
        let mut surveyors = Vec::new();
        for (id, _) in candidates.into_iter().take(capacity) {
            surveyors.push(id);
        }
        surveyors
    }

    fn assign_harvest_order(&mut self, node: usize) -> usize {
        let Some((_, max_workers)) = self
            .salvage_nodes
            .get(node)
            .map(|node| (node.position, node.max_workers as usize))
        else {
            return 0;
        };
        let selected_surveyors: Vec<UnitId> = self
            .simulation
            .world
            .selection()
            .ids()
            .iter()
            .copied()
            .filter(|id| self.simulation.kinds.get(id) == Some(&UnitKind::Surveyor))
            .collect();
        let surveyors = if selected_surveyors.is_empty() {
            self.nearest_idle_surveyors_to_node(node)
                .into_iter()
                .take(max_workers)
                .collect()
        } else {
            selected_surveyors
        };
        if surveyors.is_empty() {
            0
        } else {
            self.assign_surveyors_to_node(node, &surveyors)
        }
    }

    fn resource_node_status_line(&self, node_index: usize) -> String {
        let Some(node) = self.salvage_nodes.get(node_index) else {
            return "RESOURCE NODE // OFFLINE".to_owned();
        };
        let kind = match node.kind {
            ResourceKind::Salvage => "SALVAGE",
            ResourceKind::Flux => "FLUX",
        };
        let extracting_workers = self
            .harvest_jobs
            .values()
            .filter(|job| job.node == node_index && matches!(job.phase, HarvestPhase::Extracting))
            .count();
        let extraction_rate = (extracting_workers as f32 * HARVEST_RATE_PER_SECOND).round() as u32;
        format!(
            "{kind} {} LEFT // W{}/{} // +{:02}/S",
            node.remaining,
            self.workers_at_node(node_index),
            node.max_workers,
            extraction_rate
        )
    }

    fn friendly_unit_at(&self, point: Vec2) -> bool {
        self.simulation.world.units().iter().any(|unit| {
            unit.faction == PLAYER
                && unit.alive()
                && unit.position.distance(point)
                    <= (unit.radius * FRIENDLY_HOVER_RADIUS_SCALE).max(58.0)
        })
    }

    fn friendly_unit_id_at(&self, point: Vec2) -> Option<UnitId> {
        self.simulation
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == PLAYER && unit.alive())
            .filter_map(|unit| {
                let distance = unit.position.distance(point);
                let radius = (unit.radius * FRIENDLY_CLICK_RADIUS_SCALE).max(60.0);
                (distance <= radius).then_some((unit.id, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(id, _)| id)
    }

    /// Implements the classic RTS Ctrl-click gesture without making the
    /// renderer know about the engine's private selection buffer. The clicked
    /// unit chooses the role, then the engine filters the living player roster
    /// and preserves Shift's additive modifier.
    fn select_all_player_units_of_kind(&mut self, clicked: UnitId, additive: bool) -> bool {
        let Some(kind) = self.simulation.kinds.get(&clicked).copied() else {
            return false;
        };
        let ids: Vec<UnitId> = self
            .simulation
            .world
            .units()
            .iter()
            .filter(|unit| {
                unit.faction == PLAYER
                    && unit.alive()
                    && self.simulation.kinds.get(&unit.id).copied() == Some(kind)
            })
            .map(|unit| unit.id)
            .collect();
        let selected = self.simulation.world.select_ids(&ids, PLAYER, additive);
        if selected > 0 {
            self.status = Some((format!("{} {} SELECTED", ids.len(), kind.label()), 1.6));
        }
        !ids.is_empty()
    }

    fn issue_move_order(&mut self, destination: Vec2) {
        self.simulation.issue_move_order(destination);
        self.order_marker = Some((destination, 0.65));
    }

    fn structure_position(&self, structure: StructureKind) -> Option<Vec2> {
        match structure {
            StructureKind::Relay(index) => self
                .simulation
                .relays
                .get(index)
                .map(|relay| relay.position),
            StructureKind::Fabricator => Some(self.fabricator_position),
            StructureKind::Reactor => self.reactor_position,
        }
    }

    fn fabricator_kind_for_card(index: usize) -> Option<UnitKind> {
        match index {
            0 => Some(UnitKind::Warden),
            1 => Some(UnitKind::Engineer),
            2 => Some(UnitKind::Surveyor),
            _ => None,
        }
    }

    /// Mirrors the admission order in `MissionSimulation::queue_unit` so a
    /// disabled card row tells the player the first actionable blocker. This
    /// intentionally reads public ledgers only; the simulation still owns
    /// every spend, reservation, and command-side race.
    fn fabricator_build_gate(&self, kind: UnitKind) -> FabricatorBuildGate {
        let friendly_count = self
            .simulation
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == PLAYER && unit.alive())
            .count();
        if friendly_count + self.simulation.production.items().len() >= 12 {
            return FabricatorBuildGate::UnitCap;
        }
        if self.simulation.supply.available() < kind.supply_cost() {
            return FabricatorBuildGate::Supply;
        }
        if !self.simulation.power.is_powered(FABRICATOR_NODE) {
            return FabricatorBuildGate::Offline;
        }
        if self.simulation.flux < kind.resource_cost().secondary {
            return FabricatorBuildGate::Flux;
        }
        if self.simulation.production.items().len() >= 5 {
            return FabricatorBuildGate::QueueFull;
        }
        if self.simulation.resources.amount() < kind.resource_cost().primary {
            return FabricatorBuildGate::Salvage;
        }
        FabricatorBuildGate::Ready
    }

    fn fabricator_build_copy(&self, index: usize) -> String {
        let Some(kind) = Self::fabricator_kind_for_card(index) else {
            return self.command_card_label(index).to_owned();
        };
        let key = match index {
            0 => "Q",
            1 => "E",
            2 => "F",
            _ => "?",
        };
        let cost = kind.resource_cost();
        match self.fabricator_build_gate(kind) {
            FabricatorBuildGate::Ready => self.command_card_label(index).to_owned(),
            FabricatorBuildGate::UnitCap => format!("{key}  {} // UNIT CAP", kind.label()),
            FabricatorBuildGate::Supply => format!("{key}  {} // SUPPLY FULL", kind.label()),
            FabricatorBuildGate::Offline => format!("{key}  {} // OFFLINE", kind.label()),
            FabricatorBuildGate::Flux => {
                format!("{key}  {} // NEED {} FLUX", kind.label(), cost.secondary)
            }
            FabricatorBuildGate::QueueFull => format!("{key}  {} // QUEUE FULL", kind.label()),
            FabricatorBuildGate::Salvage => {
                // The production column shares a 250px row with HOLD/STOP;
                // keep the currency token compact so the disabled gate has a
                // deliberate gutter before the utility column.
                format!("{key}  {} // SALV {}", kind.label(), cost.primary)
            }
        }
    }

    fn fabricator_card_title(&self) -> String {
        if !self.simulation.power.is_powered(FABRICATOR_NODE) {
            "FABRICATOR // OFFLINE".to_owned()
        } else if let Some(item) = self.simulation.production.items().front() {
            let label = UnitKind::from_product(item.product)
                .map(UnitKind::label)
                .unwrap_or("UNKNOWN");
            format!(
                "FABRICATOR // BUILD {label} {:02}% // Q{}/5",
                (item.progress() * 100.0).clamp(0.0, 100.0).round() as u32,
                self.simulation.production.items().len()
            )
        } else if self.simulation.production.items().len() >= 5 {
            "FABRICATOR // QUEUE FULL".to_owned()
        } else {
            "FABRICATOR // Q/E/F QUEUE".to_owned()
        }
    }

    /// Formats the right half of the Fabricator split row (`D`) from the
    /// simulation's authoritative module state. The left half (`B`) remains
    /// a placement action, so this copy is intentionally compact enough to
    /// share one row without implying that both actions have the same gate.
    fn fabricator_module_copy(&self) -> String {
        if !self.simulation.power.is_powered(FABRICATOR_NODE) {
            return "D MOD // OFFLINE".to_owned();
        }
        if let Some(percent) = self.simulation.supply_module_percent() {
            // The split B/D row is only 250 logical pixels wide at the
            // reference HUD scale. Keep the progress token compact so it
            // remains fully visible on the 1280px native viewport.
            return format!("D MOD // {percent:02}%");
        }
        if self.simulation.supply_module_level >= 3 {
            return "D MOD // MAXED".to_owned();
        }
        if self.simulation.resources.amount() < 100 {
            // The B/D split row shares the same compact width as production
            // commands. The global telemetry already labels this ledger as
            // SALVAGE, so keep the gate to a short, unambiguous amount token.
            return "D MOD // 100".to_owned();
        }
        "D MOD 100".to_owned()
    }

    fn structure_status_line(&self, structure: StructureKind) -> String {
        // Keep this as a compact building chip rather than a sentence. The
        // command card is persistent and bitmap text has no clipping layer;
        // a bounded token layout keeps power, queue, and health readable at
        // the smallest supported browser viewport.
        let health = self
            .simulation
            .structure(structure)
            .map(|state| format!("HP{:.0}/{:.0}", state.health, state.max_health))
            .unwrap_or_else(|| "HP--/--".to_owned());
        match structure {
            StructureKind::Relay(index) => match self.simulation.relays.get(index) {
                Some(relay) if relay.active => format!("RELAY {} // ONLINE {}", index + 1, health),
                Some(relay) => format!(
                    "RELAY {} // CHARGING {:.0}% {}",
                    index + 1,
                    (relay.progress / 3.0 * 100.0).clamp(0.0, 100.0),
                    health,
                ),
                None => "RELAY".to_owned(),
            },
            StructureKind::Fabricator => {
                let module = self
                    .simulation
                    .supply_module_percent()
                    .map(|percent| {
                        format!(
                            "M{}/3 {:02}%",
                            self.simulation.supply_module_level + 1,
                            percent
                        )
                    })
                    .unwrap_or_else(|| format!("M{}/3", self.simulation.supply_module_level));
                format!(
                    "FABRICATOR // {} Q{}/5 {} {} {}",
                    if self.simulation.power.is_powered(FABRICATOR_NODE) {
                        "POWERED"
                    } else {
                        "OFFLINE"
                    },
                    self.simulation.production.items().len(),
                    if self.simulation.rally_point.is_some() {
                        "RALLY"
                    } else {
                        "NO-RALLY"
                    },
                    module,
                    health,
                )
            }
            StructureKind::Reactor => format!(
                "REACTOR // {} {}",
                if self
                    .simulation
                    .tech
                    .is_unlocked(crate::simulation::TECH_RELAY_NETWORK)
                {
                    "LATTICE ONLINE"
                } else {
                    "LATTICE LOCKED"
                },
                health,
            ),
        }
    }

    /// Formats the authored resource objective for a compact tactical chip.
    /// The simulation owns the contract and state; this method only maps that
    /// state to bounded player-facing copy and an accent color.
    fn resource_objective_hud_copy(&self) -> Option<(Vec2, String, Color)> {
        let (objective, target) = self.simulation.resource_objective_contract()?;
        let state = self.simulation.resource_objective_state()?;
        let percent = if objective.required_seconds.is_finite() && objective.required_seconds > 0.0
        {
            (state.progress_seconds / objective.required_seconds * 100.0)
                .clamp(0.0, 100.0)
                .round() as u32
        } else {
            0
        };
        let node_label = format!("NODE {}", objective.node_index.saturating_add(1));
        let (copy, accent) = if state.completed {
            (
                format!("{node_label} // SECURED"),
                Color::rgb(0.3, 1.5, 1.0),
            )
        } else if state.contested {
            (
                format!("{node_label} // CONTESTED {percent:02}%"),
                Color::rgb(1.3, 0.35, 0.4),
            )
        } else if percent > 0 {
            (
                format!("{node_label} // SECURING {percent:02}%"),
                Color::rgb(0.3, 1.25, 1.1),
            )
        } else {
            (
                format!("{node_label} // SEND {} 00%", objective.worker_kind.label()),
                Color::rgb(1.05, 0.72, 0.28),
            )
        };
        const MAX_CHARS: usize = 32;
        let bounded = if copy.chars().count() <= MAX_CHARS {
            copy
        } else {
            let mut clipped: String = copy.chars().take(MAX_CHARS - 2).collect();
            clipped.push_str("..");
            clipped
        };
        Some((target, bounded, accent))
    }

    /// Resolves the next player-facing objective from the same mission state
    /// that determines victory. The result drives the HUD, minimap, world
    /// beacon, and camera-focus key so those surfaces cannot disagree.
    fn next_objective(&self) -> Option<(Vec2, String)> {
        if let Some(objective) = self.mission.specialist_objective {
            if !self.specialist_objective_state.completed {
                return Some((
                    objective.target,
                    format!(
                        "{} // HOLD {:02}%",
                        objective.kind.objective_label(),
                        (self.specialist_objective_state.fraction(objective) * 100.0).round()
                            as u32
                    ),
                ));
            }
        }
        if let Some(objective) = self.mission.engineer_repair_objective {
            if !self.specialist_objective_state.completed {
                return Some((
                    objective.target,
                    format!(
                        "ENGINEER REPAIR // HOLD {:02}%",
                        (self
                            .specialist_objective_state
                            .engineer_repair_fraction(objective)
                            * 100.0)
                            .round() as u32
                    ),
                ));
            }
        }
        // Resource objectives are campaign beats, not just passive economy
        // telemetry. Keep them behind the specialist jobs above so Garden's
        // repair instruction remains the first thing the player sees, then
        // expose the authored node before the generic escort/victory target.
        // Reusing the compact chip copy keeps the beacon, minimap, and R-focus
        // label in lockstep with the resource objective's live state.
        if self
            .simulation
            .resource_objective_state()
            .is_some_and(|state| !state.completed)
        {
            if let Some((target, copy, _)) = self.resource_objective_hud_copy() {
                return Some((target, format!("RESOURCE // {copy}")));
            }
        }
        match self.mission.victory {
            VictoryCondition::RestoreRelaysAndDefeatBoss { boss_kind } => {
                if let Some((index, relay)) = self
                    .simulation
                    .relays
                    .iter()
                    .enumerate()
                    .find(|(_, relay)| !relay.active)
                {
                    return Some((
                        relay.position,
                        format!("RESTORE RELAY {} — ENGINEER REQUIRED", index + 1),
                    ));
                }
                self.simulation
                    .world
                    .units()
                    .iter()
                    .find(|unit| {
                        unit.faction == CHOIR
                            && unit.alive()
                            && self.simulation.kinds.get(&unit.id) == Some(&boss_kind)
                    })
                    .map(|unit| (unit.position, format!("ELIMINATE {}", boss_kind.label())))
            }
            VictoryCondition::EscortToExtraction { point, .. } => {
                Some((point, "ESCORT SENA TO THE ARRAY".to_owned()))
            }
        }
    }

    fn focus_next_objective(&mut self, ctx: &mut FrameCtx<'_>) {
        let Some((position, label)) = self.next_objective() else {
            return;
        };
        ctx.renderer.camera.position = position;
        ctx.renderer
            .camera
            .clamp_to_bounds(Aabb::from_center_size(Vec2::ZERO, MAP_SIZE));
        self.order_marker = Some((position, 1.5));
        self.target_feedback = Some((position, label.clone(), 3.0));
        self.status = Some((format!("OBJECTIVE FOCUS — {label}"), 2.5));
    }

    fn focus_last_transmission(&mut self, ctx: &mut FrameCtx<'_>) {
        let Some(position) = self.last_transmission else {
            self.status = Some(("NO RECENT TRANSMISSION".to_owned(), 1.8));
            return;
        };
        ctx.renderer.camera.position = position;
        ctx.renderer
            .camera
            .clamp_to_bounds(Aabb::from_center_size(Vec2::ZERO, MAP_SIZE));
        self.order_marker = Some((position, 1.5));
        self.target_feedback = Some((position, "TRANSMISSION".to_owned(), 3.0));
        self.status = Some(("TRANSMISSION FOCUS // SPACE".to_owned(), 2.0));
    }

    fn handle_pointer(&mut self, ctx: &mut FrameCtx<'_>) {
        let mouse_world = ctx
            .renderer
            .camera
            .screen_to_world(ctx.input.mouse_position);
        if ctx.input.mouse_pressed(MouseButton::Left) {
            if let Some(destination) = self
                .minimap_transform(ctx.renderer)
                .panel_to_world(mouse_world)
            {
                ctx.renderer.camera.position = destination;
                ctx.renderer
                    .camera
                    .clamp_to_bounds(Aabb::from_center_size(Vec2::ZERO, MAP_SIZE));
                return;
            }
            if self.placing_beacon {
                let beacon_cost = self.beacon_cost();
                match self.placement_rules().validate(mouse_world, 54.0) {
                    Ok(()) if self.simulation.resources.spend(beacon_cost) => {
                        let builders: Vec<UnitId> = self
                            .simulation
                            .world
                            .selection()
                            .ids()
                            .iter()
                            .copied()
                            .filter(|id| self.simulation.kinds.get(id) == Some(&UnitKind::Engineer))
                            .collect();
                        for builder in builders {
                            self.build_flash.insert(builder, 1.6);
                        }
                        self.field_beacons.push(FieldBeacon {
                            position: mouse_world,
                        });
                        self.placing_beacon = false;
                        self.status = Some(("FIELD BEACON DEPLOYED".to_owned(), 3.0));
                        ctx.audio.collect();
                    }
                    Ok(()) => {
                        self.status = Some((format!("BEACON REQUIRES {beacon_cost} SALVAGE"), 3.0));
                    }
                    Err(reason) => {
                        let reason = match reason {
                            PlacementError::OutsideBuildArea => "OUTSIDE BUILD AREA",
                            PlacementError::TooFarFromPower => "NO POWER LINK",
                            PlacementError::Obstructed => "PLACEMENT OBSTRUCTED",
                        };
                        self.status = Some((reason.to_owned(), 2.5));
                    }
                }
                return;
            }
            self.drag = Some(SelectionBox::begin(mouse_world));
        }
        if let Some(drag) = self.drag.as_mut() {
            drag.update(mouse_world);
        }
        if ctx.input.mouse_released(MouseButton::Left) {
            if let Some(drag) = self.drag.take() {
                if drag.start.distance(drag.current) < 18.0 {
                    if let Some(structure) = self.structure_at(mouse_world) {
                        self.selected_structure = Some(structure);
                        self.selected_resource_node = None;
                        self.simulation.world.clear_selection();
                        self.reset_command_card_page();
                    } else if let Some(node) = self.salvage_node_at(mouse_world) {
                        self.selected_structure = None;
                        if self.selected_single_unit_kind() == Some(UnitKind::Surveyor) {
                            self.selected_resource_node = None;
                            self.reset_command_card_page();
                            let assigned = self.assign_harvest_order(node);
                            self.status = Some(if assigned > 0 {
                                (format!("{assigned} SURVEYOR // RESOURCE ROUTE SET"), 2.5)
                            } else {
                                ("RESOURCE NODE SATURATED OR DRY".to_owned(), 2.5)
                            });
                        } else {
                            self.selected_resource_node = Some(node);
                            self.simulation.world.clear_selection();
                            self.reset_command_card_page();
                        }
                    } else if !ctx.input.shift_down()
                        && !self.simulation.world.selection().ids().is_empty()
                        && !self.friendly_unit_at(mouse_world)
                    {
                        // A selected squad can also use the intuitive
                        // select-then-left-click terrain command. This keeps
                        // browser play viable when a secondary click is
                        // intercepted by the host platform.
                        self.selected_structure = None;
                        self.selected_resource_node = None;
                        self.reset_command_card_page();
                        self.issue_move_order(mouse_world);
                        ctx.audio.collect();
                    } else {
                        self.selected_structure = None;
                        self.selected_resource_node = None;
                        self.reset_command_card_page();
                        let additive = ctx.input.shift_down();
                        if ctx.input.control_down() {
                            if let Some(clicked) = self.friendly_unit_id_at(mouse_world) {
                                if self.select_all_player_units_of_kind(clicked, additive) {
                                    return;
                                }
                            }
                        }
                        self.simulation
                            .world
                            .select_point(mouse_world, PLAYER, additive);
                    }
                } else {
                    self.selected_structure = None;
                    self.selected_resource_node = None;
                    self.reset_command_card_page();
                    self.simulation.world.select_bounds(
                        drag.bounds(),
                        PLAYER,
                        ctx.input.shift_down(),
                    );
                }
            }
        }
        if ctx.input.mouse_pressed(MouseButton::Right)
            && matches!(self.selected_structure, Some(StructureKind::Fabricator))
            && !self.attack_move_mode
            && !self.patrol_mode
            && !self.follow_mode
        {
            self.simulation.set_rally_point(mouse_world);
            self.status = Some(("FABRICATOR RALLY POINT SET".to_owned(), 2.5));
            self.order_marker = Some((mouse_world, 0.85));
            ctx.audio.collect();
        }
        if ctx.input.mouse_pressed(MouseButton::Right)
            && !self.simulation.world.selection().ids().is_empty()
        {
            let selected_ids = self.simulation.world.selection().ids().to_vec();
            if self.attack_move_mode {
                self.simulation
                    .issue_attack_move_order(mouse_world, ctx.input.shift_down());
                self.attack_move_mode = false;
                self.status = Some(("ATTACK-MOVE ORDERED".to_owned(), 2.0));
            } else if self.patrol_mode {
                self.simulation
                    .issue_patrol_order(mouse_world, ctx.input.shift_down());
                self.patrol_mode = false;
                self.status = Some(("PATROL ROUTE SET".to_owned(), 2.0));
            } else if self.follow_mode {
                if let Some(target) = self.friendly_unit_id_at(mouse_world) {
                    self.simulation
                        .world
                        .issue_follow(target, ctx.input.shift_down());
                    self.follow_mode = false;
                    self.status = Some(("FOLLOW ORDERED".to_owned(), 2.0));
                } else {
                    self.status = Some(("FOLLOW TARGET NOT FOUND".to_owned(), 2.0));
                }
            } else if let Some(node) = self.salvage_node_at(mouse_world) {
                let assigned = self.assign_harvest_order(node);
                if assigned > 0 {
                    self.status = Some((format!("{assigned} SURVEYOR // SALVAGE ROUTE SET"), 2.5));
                } else {
                    self.status =
                        Some(("NO SURVEYOR AVAILABLE // G KEY TO FIND ONE".to_owned(), 2.5));
                }
            } else if let Some(enemy) = self.closest_enemy_at(mouse_world) {
                self.simulation
                    .world
                    .issue_attack_order(enemy, ctx.input.shift_down());
                for id in &selected_ids {
                    self.simulation.player_paths.remove(id);
                }
            } else if ctx.input.shift_down() {
                self.simulation.queue_move_order(mouse_world);
                self.order_marker = Some((mouse_world, 0.65));
                self.status = Some(("WAYPOINT QUEUED".to_owned(), 2.0));
            } else {
                self.issue_move_order(mouse_world);
            }
            self.order_marker = Some((mouse_world, 0.65));
            ctx.audio.collect();
        }
    }

    fn update_camera(&mut self, ctx: &mut FrameCtx<'_>, dt: f32) {
        let viewport = ctx.renderer.camera.viewport();
        let mouse = ctx.input.mouse_position;
        let mut pan =
            ctx.input
                .axis_from_keys(KeyCode::KeyW, KeyCode::KeyS, KeyCode::KeyA, KeyCode::KeyD);
        const EDGE: f32 = 20.0;
        // A pointer inherited from another app often starts at (0, 0). Give
        // the deployment framing a short grace period so that edge-scroll
        // cannot immediately hide the starting roster beneath the HUD.
        let pointer_in_view = mouse.x > 1.0
            && mouse.y > 1.0
            && mouse.x < viewport.x - 1.0
            && mouse.y < viewport.y - 1.0;
        if !self.briefing && !self.mission_select && self.mission_time > 1.5 && pointer_in_view {
            if mouse.x < EDGE {
                pan.x -= 1.0;
            } else if mouse.x > viewport.x - EDGE {
                pan.x += 1.0;
            }
            if mouse.y < EDGE {
                pan.y += 1.0;
            } else if mouse.y > viewport.y - EDGE {
                pan.y -= 1.0;
            }
        }
        if pan.length_squared() > 1.0 {
            pan = pan.normalize();
        }
        ctx.renderer.camera.position += pan * (540.0 / ctx.renderer.camera.zoom) * dt;
        if ctx.input.scroll.abs() > f32::EPSILON {
            ctx.renderer
                .camera
                .zoom_at(1.0 + ctx.input.scroll * 0.09, ctx.input.mouse_position);
        }
        ctx.renderer
            .camera
            .clamp_to_bounds(Aabb::from_center_size(Vec2::ZERO, MAP_SIZE));
    }

    /// The deployment handoff should put the authored roster above the HUD.
    /// The minimap remains a useful recovery tool, but it should not be the
    /// first way a player has to discover their own units.
    fn focus_player_roster(&mut self, ctx: &mut FrameCtx<'_>) {
        let mut center = Vec2::ZERO;
        let mut count = 0.0;
        for unit in self
            .simulation
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == PLAYER && unit.alive())
        {
            center += unit.position;
            count += 1.0;
        }
        if count == 0.0 {
            return;
        }
        center /= count;
        // Move the camera slightly below the roster so the lower HUD becomes
        // a command surface beneath the units rather than a hiding place.
        ctx.renderer.camera.position = center + Vec2::new(0.0, -120.0);
        ctx.renderer
            .camera
            .clamp_to_bounds(Aabb::from_center_size(Vec2::ZERO, MAP_SIZE));
    }

    fn update_enemy_ai(&mut self, dt: f32) {
        self.enemy_think -= dt;
        if self.enemy_think > 0.0 {
            return;
        }
        self.enemy_think = 0.65;
        // Let the player read the battlefield and issue an opening order before
        // the Choir begins reacting. The first 90 seconds use a tighter leash
        // and one attacker per target so the opening teaches target priority
        // instead of collapsing into a six-unit dogpile.
        if self.mission_time < 12.0
            || (!self.simulation.relays.is_empty() && !self.simulation.relays[0].active)
        {
            return;
        }
        let params = if self.mission_time < 90.0 {
            AiParams {
                aggro_radius: 360.0,
                retarget_interval: 2.5,
                retreat_health_fraction: 0.35,
                retreat_duration: 5.0,
                max_attackers_per_target: 1,
            }
        } else {
            AiParams::default()
        };
        self.enemy_ai.think(
            &mut self.simulation.world,
            CHOIR,
            PLAYER,
            self.mission_time,
            &params,
            Some(&self.simulation.nav),
        );
    }

    /// Idle Lantern combat units acquire nearby visible threats on their own.
    /// Explicit Move and Attack orders remain authoritative, so players can
    /// still disengage or focus-fire without the automation fighting them.
    fn update_auto_targeting(&mut self) {
        let enemies: Vec<(UnitId, Vec2)> = self
            .simulation
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == CHOIR && unit.alive())
            .filter(|unit| self.fog.state_at(unit.position) == FogState::Visible)
            .map(|unit| (unit.id, unit.position))
            .collect();
        let orders: Vec<(UnitId, UnitId)> = self
            .simulation
            .world
            .units()
            .iter()
            .filter(|unit| {
                unit.faction == PLAYER
                    && unit.alive()
                    && matches!(unit.order, UnitOrder::Idle | UnitOrder::Hold)
                    && matches!(
                        self.simulation.kinds.get(&unit.id),
                        Some(UnitKind::Warden | UnitKind::Surveyor)
                    )
            })
            .filter_map(|unit| {
                let range = self.simulation.kinds.get(&unit.id)?.combat().range * 1.15;
                enemies
                    .iter()
                    .filter_map(|(id, position)| {
                        let distance = unit.position.distance(*position);
                        (distance <= range).then_some((*id, distance))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(target, _)| (unit.id, target))
            })
            .collect();
        for (unit, target) in orders {
            if let Some(unit) = self.simulation.world.unit_mut(unit) {
                unit.order = UnitOrder::Attack(target);
            }
        }
    }

    fn update_harvesting(&mut self, dt: f32) -> u32 {
        const CARGO_CAPACITY: u32 = 24;
        const INTERACT_RANGE: f32 = 105.0;
        let ids: Vec<UnitId> = self.harvest_jobs.keys().copied().collect();
        let mut deposited = 0;
        let mut deposited_flux = 0;
        let mut remove = Vec::new();
        for id in ids {
            let Some(position) = self
                .simulation
                .world
                .unit(id)
                .filter(|unit| unit.alive())
                .map(|unit| unit.position)
            else {
                remove.push(id);
                continue;
            };
            let Some(snapshot) = self.harvest_jobs.get(&id).copied() else {
                continue;
            };
            match snapshot.phase {
                HarvestPhase::ToNode => {
                    let Some(node_position) = self
                        .salvage_nodes
                        .get(snapshot.node)
                        .filter(|node| node.remaining > 0)
                        .map(|node| node.position)
                    else {
                        if !self.continue_harvest_route(id, position, Some(snapshot.node)) {
                            remove.push(id);
                        }
                        continue;
                    };
                    if position.distance(node_position) <= INTERACT_RANGE {
                        if let Some(job) = self.harvest_jobs.get_mut(&id) {
                            job.phase = HarvestPhase::Extracting;
                        }
                    }
                }
                HarvestPhase::Extracting => {
                    let Some(node) = self.salvage_nodes.get_mut(snapshot.node) else {
                        remove.push(id);
                        continue;
                    };
                    node.harvest_buffer += dt.max(0.0) * HARVEST_RATE_PER_SECOND;
                    let available = node.harvest_buffer.floor() as u32;
                    let space = CARGO_CAPACITY.saturating_sub(snapshot.cargo);
                    let amount = available.min(space).min(node.remaining);
                    if amount > 0 {
                        node.harvest_buffer -= amount as f32;
                        node.remaining -= amount;
                        if let Some(job) = self.harvest_jobs.get_mut(&id) {
                            job.cargo += amount;
                        }
                    }
                    let cargo = snapshot.cargo + amount;
                    if cargo >= CARGO_CAPACITY || node.remaining == 0 {
                        if let Some(job) = self.harvest_jobs.get_mut(&id) {
                            job.phase = HarvestPhase::ToDepot;
                        }
                        self.simulation
                            .issue_unit_move(id, self.fabricator_position);
                    }
                }
                HarvestPhase::ToDepot => {
                    if position.distance(self.fabricator_position) <= 135.0 {
                        let node_kind = self
                            .salvage_nodes
                            .get(snapshot.node)
                            .map(|node| node.kind)
                            .unwrap_or(ResourceKind::Salvage);
                        if node_kind == ResourceKind::Flux {
                            deposited_flux += snapshot.cargo;
                        } else {
                            deposited += snapshot.cargo;
                        }
                        let node_position = self
                            .salvage_nodes
                            .get(snapshot.node)
                            .filter(|node| node.remaining > 0)
                            .map(|node| node.position);
                        if let Some(job) = self.harvest_jobs.get_mut(&id) {
                            job.cargo = 0;
                            job.phase = HarvestPhase::ToNode;
                        }
                        if let Some(node_position) = node_position {
                            self.simulation.issue_unit_move(id, node_position);
                        } else if !self.continue_harvest_route(id, position, Some(snapshot.node)) {
                            remove.push(id);
                        }
                    }
                }
            }
        }
        for id in remove {
            self.harvest_jobs.remove(&id);
        }
        if deposited > 0 {
            self.simulation.credit_salvage(deposited);
        }
        if deposited_flux > 0 {
            self.simulation.credit_flux(deposited_flux);
        }
        deposited
    }

    /// Advances the current mission's authored role objective from live unit
    /// positions. This keeps the contract deterministic and lets the normal
    /// objective beacon/HUD teach each role's job without adding a bespoke
    /// simulation victory condition.
    fn update_specialist_objective(&mut self, dt: f32) {
        let (target, required_unit) = if let Some(objective) = self.mission.specialist_objective {
            (objective.target, objective.kind.required_unit())
        } else if let Some(objective) = self.mission.engineer_repair_objective {
            (objective.target, objective.required_unit())
        } else {
            return;
        };
        if self.specialist_objective_state.completed {
            return;
        }
        let Some((unit_kind, unit_position)) = self
            .simulation
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == PLAYER && unit.alive())
            .filter_map(|unit| {
                let kind = self.simulation.kinds.get(&unit.id).copied()?;
                (kind == required_unit).then_some((kind, unit.position))
            })
            .min_by(|left, right| {
                left.1
                    .distance_squared(target)
                    .total_cmp(&right.1.distance_squared(target))
            })
        else {
            return;
        };
        let was_complete = self.specialist_objective_state.completed;
        let completed = if let Some(objective) = self.mission.specialist_objective {
            self.specialist_objective_state
                .advance(objective, unit_kind, unit_position, dt)
        } else if let Some(objective) = self.mission.engineer_repair_objective {
            self.specialist_objective_state.advance_engineer_repair(
                objective,
                unit_kind,
                unit_position,
                dt,
            )
        } else {
            false
        };
        if completed && !was_complete {
            let (status, speaker, text) = if let Some(objective) = self.mission.specialist_objective
            {
                match objective.kind {
                    SpecialistObjectiveKind::SurveyorScan => (
                        "SURVEY ARRAY STABILIZED // ESCORT WINDOW OPEN",
                        "SENA QUILL",
                        "THE ARRAY HAS A LOCK. HOLD THE LINE WHILE I FOLLOW THE SIGNAL.",
                    ),
                    SpecialistObjectiveKind::WardenHold => (
                        "RELAY APRON SECURED // PUSH WINDOW OPEN",
                        "MARA VEY",
                        "THE RELAY IS STABLE. WARDENS FORWARD—ENGINEERS CAN WORK THE NEXT LINK.",
                    ),
                }
            } else {
                (
                    "REACTOR REPAIRED // EXTRACTION WINDOW OPEN",
                    "IVO ROOK",
                    "THE REACTOR IS HOLDING. I CAN KEEP THE LANTERN FED WHILE SENA TAKES THE ARRAY.",
                )
            };
            self.status = Some((status.to_owned(), 4.0));
            self.queue_urgent_radio_line(speaker, text, Some(target));
        }
    }

    /// Samples the live roster and authored map for the terrain-control
    /// contract. Keeping this snapshot in one helper makes the progression
    /// hook and HUD copy consume identical unit/terrain/contest facts.
    fn terrain_control_presence(
        &self,
        objective: TerrainControlObjective,
    ) -> TerrainControlPresence {
        let nearest = self
            .simulation
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == PLAYER && unit.alive())
            .filter_map(|unit| {
                let kind = self.simulation.kinds.get(&unit.id).copied()?;
                (kind == objective.required_unit).then_some((kind, unit.position))
            })
            .min_by(|left, right| {
                left.1
                    .distance_squared(objective.target)
                    .total_cmp(&right.1.distance_squared(objective.target))
            });
        let (unit_kind, unit_position) =
            nearest.unwrap_or((objective.required_unit, Vec2::splat(1_000_000.0)));
        let (terrain_elevation, terrain_cover) =
            TerrainZone::resolve_at(unit_position, &self.mission.terrain_zones)
                .map(|(_, zone)| (zone.elevation, zone.normalized_cover()))
                .unwrap_or((0, 0.0));
        let enemy_present = self.simulation.world.units().iter().any(|unit| {
            unit.faction == CHOIR
                && unit.alive()
                && unit.position.distance(objective.target) <= objective.radius * 1.15
        });
        TerrainControlPresence {
            unit_kind,
            unit_position,
            terrain_elevation,
            terrain_cover,
            enemy_present,
        }
    }

    /// Advances the authored ridge beat without changing the generic mission
    /// victory condition. Completion is a campaign handoff, not an instant
    /// win: the player still has to finish the relay/boss objective.
    fn update_terrain_control_objective(&mut self, dt: f32) {
        let Some(objective) = self.mission.terrain_control_objective else {
            return;
        };
        let was_complete = self.terrain_control_state.completed;
        let presence = self.terrain_control_presence(objective);
        let outcome = self.terrain_control_state.advance(objective, presence, dt);
        if outcome == TerrainControlAdvance::Completed && !was_complete {
            self.status = Some(("HIGH GROUND SECURED // FIRING ANGLE OPEN".to_owned(), 4.0));
            self.queue_urgent_radio_line(
                "MARA VEY",
                "HIGH GROUND IS OURS. THE CHOIR HAS TO CLIMB INTO OUR FIRE.",
                Some(objective.target),
            );
        }
    }

    fn terrain_control_progress_line(&self) -> Option<String> {
        let objective = self.mission.terrain_control_objective?;
        let presence = self.terrain_control_presence(objective);
        let percent = (self.terrain_control_state.fraction(objective) * 100.0).round() as u32;
        if self.terrain_control_state.completed {
            return Some("RIDGE SECURED // HIGH GROUND".to_owned());
        }
        let state = if self.terrain_control_state.contested {
            "CONTESTED"
        } else if !objective.unit_present(presence) {
            "SEND WARDEN"
        } else if !objective.terrain_satisfies(presence.terrain_elevation, presence.terrain_cover) {
            "MOVE HIGH GROUND"
        } else {
            "HOLDING"
        };
        Some(format!("RIDGE HOLD {percent:02}% // {state}"))
    }

    fn mission_objective_progress_line(&self) -> Option<String> {
        self.terrain_control_progress_line()
            .or_else(|| self.specialist_objective_progress_line())
    }

    fn specialist_objective_progress_line(&self) -> Option<String> {
        if let Some(objective) = self.mission.specialist_objective {
            if self.specialist_objective_state.completed {
                // Keep the completion handoff visible in the same telemetry slot
                // after the specialist's portrait card fades. This makes each
                // authored role beat feel earned without another permanent panel.
                return Some(objective.kind.completion_label().to_owned());
            }
            return Some(format!(
                "{} {:02}%",
                objective.kind.progress_label(),
                (self.specialist_objective_state.fraction(objective) * 100.0).round() as u32
            ));
        }
        let objective = self.mission.engineer_repair_objective?;
        if self.specialist_objective_state.completed {
            return Some("REPAIR COMPLETE // EXTRACTION".to_owned());
        }
        Some(format!(
            "REPAIR REACTOR {:02}%",
            (self
                .specialist_objective_state
                .engineer_repair_fraction(objective)
                * 100.0)
                .round() as u32
        ))
    }

    fn update_radio_dialogue(&mut self, dt: f32) {
        self.radio_pop_in = (self.radio_pop_in - dt.max(0.0) * 3.0).max(0.0);
        if let Some((_, _, remaining)) = self.radio_message.as_mut() {
            *remaining -= dt.max(0.0);
            if *remaining <= 0.0 {
                self.radio_message = None;
            } else {
                return;
            }
        }
        if self.radio_message.is_none() {
            let next_line = self
                .radio_priority_queue
                .pop_front()
                .or_else(|| self.radio_queue.pop_front());
            if let Some((speaker, text, position)) = next_line {
                self.queue_radio_line(speaker, text, position);
                return;
            }
        }
        let Some(line) = self.mission.radio_lines.get(self.dialogue_cursor) else {
            return;
        };
        let active_relays = self
            .simulation
            .relays
            .iter()
            .filter(|relay| relay.active)
            .count();
        let ready = match line.trigger {
            DialogueTrigger::Time(time) => self.mission_time >= time,
            DialogueTrigger::RelaysOnline(count) => active_relays >= count,
            DialogueTrigger::SalvageDelivered(amount) => {
                self.simulation.salvage_delivered >= amount
            }
            DialogueTrigger::EnemyRaid(count) => self.simulation.enemy_raid_count >= count,
            DialogueTrigger::UnitDestroyed(kind) => self.simulation.destroyed_count(kind) > 0,
            DialogueTrigger::ResourceObjectiveCompleted => self
                .simulation
                .resource_objective_state()
                .is_some_and(|state| state.completed),
        };
        if ready {
            let position = self.next_objective().map(|(position, _)| position);
            self.queue_radio_line(line.speaker, line.text, position);
            self.dialogue_cursor += 1;
        }
    }

    /// Return whether a Surveyor is currently performing a job that owns the
    /// scan fan. Traversal to a node and the depot are movement states, not
    /// scan states, so their silhouettes stay quiet and readable.
    fn surveyor_scan_active(&self, id: UnitId) -> bool {
        if self
            .harvest_jobs
            .get(&id)
            .is_some_and(|job| matches!(job.phase, HarvestPhase::Extracting))
        {
            return true;
        }
        let Some(unit) = self.simulation.world.unit(id) else {
            return false;
        };
        self.simulation
            .scan_pulse
            .is_some_and(|(origin, remaining)| {
                remaining > 0.0 && unit.position.distance_squared(origin) <= 1.0
            })
    }

    /// Structure repair has no per-engineer event payload, so infer the
    /// active support beam from the same range and damage contract used by
    /// the deterministic simulation. The short event flash covers a repair
    /// that reaches full health on this tick.
    fn engineer_repairing(&self, id: UnitId) -> bool {
        if self.repair_flash.contains_key(&id) {
            return true;
        }
        const REPAIR_RANGE: f32 = 145.0;
        let Some(engineer) = self.simulation.world.unit(id) else {
            return false;
        };
        if engineer.faction != PLAYER || !engineer.alive() {
            return false;
        }
        let damaged_ally = self.simulation.world.units().iter().any(|candidate| {
            candidate.id != id
                && candidate.faction == PLAYER
                && candidate.alive()
                && candidate.health + f32::EPSILON < candidate.max_health
                && candidate.position.distance(engineer.position) <= REPAIR_RANGE
        });
        if damaged_ally {
            return true;
        }

        let relay_damaged = self
            .simulation
            .relays
            .iter()
            .enumerate()
            .any(|(index, relay)| {
                self.simulation
                    .structure(StructureKind::Relay(index))
                    .is_some_and(|structure| {
                        structure.health + f32::EPSILON < structure.max_health
                            && relay.position.distance(engineer.position) <= REPAIR_RANGE * 1.35
                    })
            });
        let fabricator_damaged = self
            .simulation
            .structure(StructureKind::Fabricator)
            .is_some_and(|structure| {
                structure.health + f32::EPSILON < structure.max_health
                    && self
                        .simulation
                        .fabricator_position
                        .distance(engineer.position)
                        <= REPAIR_RANGE * 1.35
            });
        let reactor_damaged = self
            .simulation
            .reactor_position
            .zip(self.simulation.structure(StructureKind::Reactor))
            .is_some_and(|(position, structure)| {
                structure.health + f32::EPSILON < structure.max_health
                    && position.distance(engineer.position) <= REPAIR_RANGE * 1.35
            });
        relay_damaged || fabricator_damaged || reactor_damaged
    }

    /// Construction is a player-facing Engineer job even before a beacon is
    /// committed. Keep the preview cue attached to the selected builder and
    /// retain it briefly after placement so the deploy action reads as work,
    /// not as a teleporting structure.
    fn engineer_building(&self, id: UnitId) -> bool {
        self.build_flash.contains_key(&id)
            || (self.placing_beacon
                && self.simulation.world.selection().contains(id)
                && self.simulation.kinds.get(&id) == Some(&UnitKind::Engineer))
    }

    fn surveyor_marking(&self, id: UnitId) -> bool {
        self.mark_flash.contains_key(&id)
    }

    fn unit_animation_state(
        &self,
        id: UnitId,
        kind: UnitKind,
        alive: bool,
        velocity: Vec2,
        engaged: bool,
    ) -> UnitAnimationState {
        if !alive {
            return UnitAnimationState::Down;
        }
        if self.damage_flash.contains_key(&id) {
            return UnitAnimationState::Hit;
        }
        match kind {
            UnitKind::Warden if engaged => UnitAnimationState::Attack,
            UnitKind::Warden if velocity.length_squared() > 1.0 => UnitAnimationState::Move,
            // Repair wins over movement: an Engineer can still have a small
            // residual velocity while its beam is active at the edge of its
            // support envelope.
            UnitKind::Engineer if self.engineer_repairing(id) => UnitAnimationState::Repair,
            UnitKind::Engineer if self.engineer_building(id) => UnitAnimationState::Build,
            UnitKind::Engineer if engaged => UnitAnimationState::Attack,
            UnitKind::Engineer if velocity.length_squared() > 1.0 => UnitAnimationState::Move,
            UnitKind::Surveyor if self.surveyor_marking(id) => UnitAnimationState::Mark,
            UnitKind::Surveyor if self.surveyor_scan_active(id) => UnitAnimationState::Scan,
            UnitKind::Surveyor if engaged => UnitAnimationState::Attack,
            UnitKind::Surveyor if velocity.length_squared() > 1.0 => UnitAnimationState::Move,
            UnitKind::Needle if engaged => UnitAnimationState::Attack,
            UnitKind::Canticle if engaged => UnitAnimationState::Command,
            UnitKind::BellMine if engaged => UnitAnimationState::Arm,
            _ => UnitAnimationState::Idle,
        }
    }

    fn unit_animation_clip(
        &self,
        kind: UnitKind,
        state: UnitAnimationState,
    ) -> Option<AnimationClip> {
        let reaction_frames =
            (kind.atlas_frame() * 4..kind.atlas_frame() * 4 + 4).collect::<Vec<_>>();
        match state {
            UnitAnimationState::Down => Some(AnimationClip::once("down", reaction_frames, 6.0)),
            UnitAnimationState::Hit => Some(AnimationClip::once("hit", reaction_frames, 14.0)),
            UnitAnimationState::Move => match kind {
                UnitKind::Warden => Some(AnimationClip::looping("move", [0, 1, 2, 3, 4, 5], 10.0)),
                UnitKind::Engineer => Some(AnimationClip::looping("move", [0, 1, 2, 3, 4, 5], 9.0)),
                UnitKind::Surveyor => {
                    Some(AnimationClip::looping("move", [0, 1, 2, 3, 4, 5], 10.0))
                }
                _ => None,
            },
            UnitAnimationState::Repair => {
                Some(AnimationClip::looping("repair", [0, 1, 2, 3, 4, 5], 12.0))
            }
            UnitAnimationState::Build => Some(AnimationClip::looping(
                "build",
                [0, 1, 2, 3, 4, 5, 6, 7],
                9.0,
            )),
            UnitAnimationState::Mark => Some(AnimationClip::looping("mark", [0, 1, 2, 3], 8.0)),
            UnitAnimationState::Scan => {
                Some(AnimationClip::looping("scan", [0, 1, 2, 3, 4, 5], 7.0))
            }
            UnitAnimationState::Attack if kind == UnitKind::Warden => {
                Some(AnimationClip::looping("attack", [0, 1, 2, 3, 4], 12.0))
            }
            UnitAnimationState::Attack => {
                Some(AnimationClip::looping("attack", [0, 1, 2, 3, 4, 5], 11.0))
            }
            UnitAnimationState::Command => {
                Some(AnimationClip::looping("command", [0, 1, 2, 3, 4, 5], 7.5))
            }
            UnitAnimationState::Arm => Some(AnimationClip::looping("arm", [0, 1, 2, 3, 4, 5], 8.5)),
            UnitAnimationState::Idle => None,
        }
    }

    fn unit_animation_atlas(
        &self,
        kind: UnitKind,
        state: UnitAnimationState,
    ) -> Option<(TextureHandle, &TextureAtlas)> {
        match (kind, state) {
            (UnitKind::Warden, UnitAnimationState::Move) => {
                Some((self.tex_warden_move, &self.warden_move_atlas))
            }
            (UnitKind::Warden, UnitAnimationState::Attack) => {
                Some((self.tex_warden_attack, &self.warden_attack_atlas))
            }
            (UnitKind::Engineer, UnitAnimationState::Move) => {
                Some((self.tex_engineer_move, &self.engineer_move_atlas))
            }
            (UnitKind::Engineer, UnitAnimationState::Repair) => {
                Some((self.tex_engineer_repair, &self.engineer_repair_atlas))
            }
            (UnitKind::Engineer, UnitAnimationState::Build) => {
                Some((self.tex_engineer_build, &self.engineer_build_atlas))
            }
            (UnitKind::Surveyor, UnitAnimationState::Move) => {
                Some((self.tex_surveyor_move, &self.surveyor_move_atlas))
            }
            (UnitKind::Surveyor, UnitAnimationState::Scan) => {
                Some((self.tex_surveyor_scan, &self.surveyor_scan_atlas))
            }
            (UnitKind::Surveyor, UnitAnimationState::Mark) => {
                Some((self.tex_surveyor_mark, &self.surveyor_mark_atlas))
            }
            (UnitKind::Needle, UnitAnimationState::Attack) => {
                Some((self.tex_needle_attack, &self.needle_attack_atlas))
            }
            (UnitKind::Canticle, UnitAnimationState::Command) => {
                Some((self.tex_canticle_command, &self.canticle_command_atlas))
            }
            (UnitKind::BellMine, UnitAnimationState::Arm) => {
                Some((self.tex_bell_mine_arm, &self.bell_mine_arm_atlas))
            }
            (_, UnitAnimationState::Hit) => {
                Some((self.tex_hit_reactions, &self.hit_reactions_atlas))
            }
            _ => None,
        }
    }

    fn unit_engaged(&self, id: UnitId) -> bool {
        let Some(unit) = self.simulation.world.unit(id) else {
            return false;
        };
        let Some(target) = self.attack_target_for_unit(id) else {
            return false;
        };
        let Some(range) = self
            .simulation
            .kinds
            .get(&id)
            .map(|kind| kind.combat().range)
        else {
            return false;
        };
        self.simulation.world.unit(target).is_some_and(|target| {
            target.alive() && unit.position.distance(target.position) <= range
        })
    }

    fn attack_target_for_unit(&self, id: UnitId) -> Option<UnitId> {
        let unit = self.simulation.world.unit(id)?;
        match unit.order {
            UnitOrder::Attack(target) => Some(target),
            UnitOrder::AttackMove(_) => self
                .simulation
                .world
                .units()
                .iter()
                .filter(|candidate| {
                    candidate.faction == CHOIR
                        && candidate.alive()
                        && unit.position.distance(candidate.position)
                            <= self.simulation.kinds[&id].combat().range
                })
                .min_by(|left, right| {
                    unit.position
                        .distance(left.position)
                        .total_cmp(&unit.position.distance(right.position))
                })
                .map(|candidate| candidate.id),
            _ => None,
        }
    }

    fn update_specialist_doctrines(&mut self, dt: f32) {
        let rescue_screen = self.specialist_module(MARA, MARA_RESCUE) == MARA_RESCUE;
        let guardian_protocol = self.lumen_protocol() == Some(LUMEN_GUARDIAN);
        let bloom_covenant = self.verdant_covenant() == Some(VERDANT_BLOOM);
        if !rescue_screen && !guardian_protocol && !bloom_covenant {
            return;
        }
        let mut sustain_sources = vec![self.fabricator_position];
        sustain_sources.extend(self.field_beacons.iter().map(|beacon| beacon.position));
        for unit in self
            .simulation
            .world
            .units_mut()
            .iter_mut()
            .filter(|unit| unit.faction == PLAYER && unit.alive())
        {
            if sustain_sources
                .iter()
                .any(|source| source.distance(unit.position) <= 300.0)
            {
                let mut healing = if rescue_screen { 3.0 } else { 0.0 }
                    + if guardian_protocol { 4.0 } else { 0.0 };
                if bloom_covenant
                    && self
                        .field_beacons
                        .iter()
                        .any(|beacon| beacon.position.distance(unit.position) <= 340.0)
                {
                    healing += 5.0;
                }
                unit.health = (unit.health + dt.max(0.0) * healing).min(unit.max_health);
            }
        }
    }

    fn update_combat_presentation(&mut self, dt: f32) {
        if self.verdant_covenant() == Some(VERDANT_BRIAR) {
            let targets: Vec<UnitId> = self
                .simulation
                .world
                .units()
                .iter()
                .filter(|unit| {
                    unit.faction == CHOIR
                        && unit.alive()
                        && self
                            .field_beacons
                            .iter()
                            .any(|beacon| beacon.position.distance(unit.position) <= 220.0)
                })
                .map(|unit| unit.id)
                .collect();
            for target in targets {
                self.simulation
                    .apply_environmental_damage(target, 8.0 * dt.max(0.0));
            }
        }
        self.attack_flash.retain(|_, flash| {
            *flash -= dt;
            *flash > 0.0
        });
        self.damage_flash.retain(|_, flash| {
            *flash -= dt;
            *flash > 0.0
        });
        self.repair_flash.retain(|_, (_, flash)| {
            *flash -= dt;
            *flash > 0.0
        });
        self.build_flash.retain(|_, flash| {
            *flash -= dt;
            *flash > 0.0
        });
        self.mark_flash.retain(|_, flash| {
            *flash -= dt;
            *flash > 0.0
        });
        for age in self.down_units.values_mut() {
            *age += dt.max(0.0);
        }
    }

    fn update_fog(&mut self) {
        self.fog.begin_frame();
        for unit in self
            .simulation
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == PLAYER && unit.alive())
        {
            let radius = if self.simulation.kinds.get(&unit.id) == Some(&UnitKind::Surveyor) {
                let base = if self.specialist_module(SENA, SENA_DEEP_SCAN) == SENA_DEEP_SCAN {
                    540.0
                } else {
                    440.0
                };
                base + if self.lumen_protocol() == Some(LUMEN_WITNESS) {
                    80.0
                } else {
                    0.0
                }
            } else {
                300.0
            };
            self.fog.reveal(unit.position, radius);
        }
        for beacon in &self.field_beacons {
            self.fog.reveal(
                beacon.position,
                if self.save_data.campaign.has_upgrade(UPGRADE_OPTICS) {
                    480.0
                } else {
                    380.0
                } + if self.lumen_protocol() == Some(LUMEN_WITNESS) {
                    80.0
                } else {
                    0.0
                },
            );
        }
        if let Some((position, remaining)) = self.simulation.scan_pulse {
            self.fog.reveal(position, 760.0 + remaining * 24.0);
        }
    }

    /// Resolve the visual state of one structure from simulation-owned facts.
    /// Damage takes precedence over power/build status so a wounded relay does
    /// not look healthy merely because its power node is online. A Fabricator
    /// module build is treated as booting even though the base structure itself
    /// is already operational.
    fn structure_visual_state(&self, kind: StructureKind) -> Option<StructureVisualState> {
        let structure = self.simulation.structure(kind)?;
        if structure.health + f32::EPSILON < structure.max_health {
            return Some(StructureVisualState::Damaged);
        }
        if kind == StructureKind::Fabricator && self.simulation.supply_module_progress.is_some() {
            return Some(StructureVisualState::Booting);
        }
        if structure.build_progress + f32::EPSILON < 1.0 {
            return Some(StructureVisualState::Booting);
        }
        if !structure.powered {
            return Some(StructureVisualState::Offline);
        }
        Some(StructureVisualState::Online)
    }

    /// Draw a small, state-aware structure effect using existing runtime
    /// textures. This deliberately stays separate from the structure atlas:
    /// dedicated offline/boot/damaged strips can replace it later without
    /// changing simulation or atlas frame numbering.
    fn draw_structure_state_fx(
        &self,
        renderer: &mut Renderer,
        kind: StructureKind,
        position: Vec2,
        size: Vec2,
        elapsed: f32,
    ) {
        let Some(state) = self.structure_visual_state(kind) else {
            return;
        };
        match state {
            StructureVisualState::Online => {}
            StructureVisualState::Offline => {
                renderer.draw_sprite(
                    self.tex_glow,
                    Sprite::new(position, size * 1.12)
                        .with_color(Color::rgba(0.12, 0.18, 0.22, 0.12))
                        .with_z(0.08),
                );
                let arm = size.x.min(size.y) * 0.34;
                for angle in [0.0, std::f32::consts::FRAC_PI_2] {
                    renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(position, Vec2::new(arm * 2.0, 4.0))
                            .with_color(Color::rgba(0.5, 0.62, 0.66, 0.42))
                            .with_rotation(angle)
                            .with_z(0.12),
                    );
                }
            }
            StructureVisualState::Booting => {
                let pulse = 0.62 + 0.24 * (elapsed * 5.0).sin().abs();
                renderer.draw_sprite(
                    self.tex_glow,
                    Sprite::new(position, size * (1.16 + pulse * 0.08))
                        .with_color(Color::rgba(0.08, 1.25, 1.12, 0.10 + pulse * 0.08))
                        .with_z(0.08),
                );
                let arm = size.x.min(size.y) * 0.38;
                for index in 0..4 {
                    let angle = elapsed * 1.8 + index as f32 * std::f32::consts::FRAC_PI_2;
                    let offset = Vec2::new(angle.cos(), angle.sin()) * arm;
                    renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(position + offset, Vec2::new(arm * 0.54, 4.0))
                            .with_color(Color::rgba(0.18, 1.3, 1.16, 0.72))
                            .with_rotation(angle)
                            .with_z(0.12),
                    );
                }
                // A compact progress track makes a relay/reactor boot state
                // legible without adding another always-on world label.
                let progress = self
                    .simulation
                    .structure(kind)
                    .map(|structure| structure.build_progress.clamp(0.0, 1.0))
                    .unwrap_or(0.0);
                let track = Vec2::new(size.x * 0.72, 5.0);
                let origin = position + Vec2::new(0.0, -size.y * 0.46);
                renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(origin, track)
                        .with_color(Color::rgba(0.01, 0.03, 0.04, 0.82))
                        .with_z(0.13),
                );
                if progress > 0.0 {
                    renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(
                            origin + Vec2::new(-track.x * 0.5 + track.x * progress * 0.5, 0.0),
                            Vec2::new(track.x * progress, 3.0),
                        )
                        .with_color(Color::rgba(0.2, 1.4, 1.18, 0.92))
                        .with_z(0.14),
                    );
                }
            }
            StructureVisualState::Damaged => {
                let pulse = 0.55 + 0.35 * (elapsed * 7.0).sin().abs();
                renderer.draw_sprite(
                    self.tex_glow,
                    Sprite::new(position, size * (1.08 + pulse * 0.08))
                        .with_color(Color::rgba(1.45, 0.08, 0.32, 0.10 + pulse * 0.10))
                        .with_z(0.08),
                );
                let arm = size.x.min(size.y) * 0.34;
                for angle in [std::f32::consts::FRAC_PI_4, -std::f32::consts::FRAC_PI_4] {
                    renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(position, Vec2::new(arm * 2.0, 5.0))
                            .with_color(Color::rgba(1.55, 0.12, 0.36, 0.78))
                            .with_rotation(angle)
                            .with_z(0.12),
                    );
                }
            }
        }
    }

    fn draw_text(
        &self,
        renderer: &mut Renderer,
        text: &str,
        origin: Vec2,
        pixel: f32,
        color: Color,
        z: f32,
    ) {
        for glyph in BitmapText::glyphs(text, origin, pixel) {
            renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(glyph.position, Vec2::splat(glyph.size))
                    .with_color(color)
                    .with_z(z),
            );
        }
    }

    /// Keeps radio copy inside its compact portrait panel. BitmapText has a
    /// fixed advance and no clipping layer, so wrapping at word boundaries is
    /// more reliable than letting a sentence bleed into the pause control.
    fn radio_line_chunks(text: &str) -> [String; 2] {
        const LIMIT: usize = 54;
        let mut lines = [String::new(), String::new()];
        for word in text.split_whitespace() {
            let target = if lines[0].is_empty() || lines[0].len() + 1 + word.len() <= LIMIT {
                &mut lines[0]
            } else {
                &mut lines[1]
            };
            if target.is_empty() {
                target.push_str(word);
            } else if target.len() + 1 + word.len() <= LIMIT {
                target.push(' ');
                target.push_str(word);
            }
        }
        if lines[1].len() > LIMIT {
            lines[1] = lines[1].chars().take(LIMIT.saturating_sub(2)).collect();
            lines[1].push_str("..");
        }
        lines
    }

    /// The mission briefing stores a speaker prefix in its authored copy
    /// (`"SENA QUILL: ..."`). Splitting that prefix here lets the briefing
    /// render a proper comms card while preserving the authored sentence as
    /// the single source of truth for the campaign script.
    fn briefing_speaker(&self) -> &str {
        self.mission
            .briefing_story
            .split_once(':')
            .map(|(speaker, _)| speaker.trim())
            .unwrap_or("MISSION CONTROL")
    }

    fn briefing_story_copy(&self) -> &str {
        self.mission
            .briefing_story
            .split_once(':')
            .map(|(_, story)| story.trim())
            .unwrap_or(self.mission.briefing_story)
    }

    /// Briefing copy gets a third line because authored mission hooks are
    /// deliberately more descriptive than the two-line in-game radio card.
    /// The hard cap prevents BitmapText from bleeding into the upgrade grid
    /// on narrow browser viewports.
    fn briefing_story_chunks(text: &str) -> [String; 3] {
        const LIMIT: usize = 62;
        let mut lines = [String::new(), String::new(), String::new()];
        let mut line = 0;
        for word in text.split_whitespace() {
            if !lines[line].is_empty() && lines[line].len() + 1 + word.len() > LIMIT {
                line += 1;
                if line >= lines.len() {
                    break;
                }
            }
            if lines[line].is_empty() {
                lines[line].push_str(word);
            } else {
                lines[line].push(' ');
                lines[line].push_str(word);
            }
        }
        if line == lines.len() - 1 && lines[line].len() >= LIMIT {
            lines[line] = lines[line].chars().take(LIMIT.saturating_sub(2)).collect();
            lines[line].push_str("..");
        }
        lines
    }

    fn speaker_role_label(speaker: &str) -> &'static str {
        match speaker {
            "MARA VEY" => "COMMAND",
            "IVO ROOK" | "IVO RENN" => "ENGINEERING",
            "SENA QUILL" => "FIELD SIGNAL",
            "OLAN VOSS" => "ANALYSIS",
            "PREFECT VALE" => "COMPACT LIAISON",
            "LUMEN" => "AWAKENED INTELLIGENCE",
            _ => "LANTERN COMMS",
        }
    }

    fn speaker_accent(speaker: &str) -> Color {
        match speaker {
            "MARA VEY" => Color::rgb(0.3, 1.4, 1.2),
            "IVO ROOK" | "IVO RENN" => Color::rgb(1.05, 0.72, 0.24),
            "SENA QUILL" => Color::rgb(0.72, 0.58, 1.25),
            "OLAN VOSS" => Color::rgb(0.52, 0.86, 1.25),
            "PREFECT VALE" => Color::rgb(1.2, 0.82, 0.55),
            "LUMEN" => Color::rgb(0.32, 1.5, 1.38),
            _ => Color::rgb(0.55, 0.8, 0.85),
        }
    }

    fn speaker_portrait_frame(speaker: &str) -> u32 {
        match speaker {
            "MARA VEY" => 0,
            "IVO ROOK" | "IVO RENN" => 1,
            "SENA QUILL" => 2,
            "OLAN VOSS" => 3,
            "PREFECT VALE" => 4,
            "LUMEN" => 5,
            _ => 0,
        }
    }

    fn unit_portrait_frame(kind: UnitKind) -> u32 {
        match kind {
            UnitKind::Warden => 0,
            UnitKind::Engineer => 1,
            UnitKind::Surveyor => 2,
            UnitKind::Needle | UnitKind::Canticle | UnitKind::BellMine => 5,
        }
    }

    fn unit_card_portrait(kind: UnitKind, hostile: bool) -> UnitCardPortrait {
        if hostile {
            UnitCardPortrait::Tactical(kind.atlas_frame())
        } else {
            UnitCardPortrait::Command(Self::unit_portrait_frame(kind))
        }
    }

    /// Returns the short alert copy shown in the persistent telemetry strip.
    /// The forecast is intentionally disclosed only during the actionable
    /// warning window; banking and cooldown are useful simulation facts but
    /// would turn the top-left HUD into a second status dashboard.
    fn raid_hud_copy(state: RaidState) -> Option<String> {
        (state.phase == RaidPhase::Warning).then(|| {
            format!(
                "RAID {:02} // {} IN {:02}s",
                state.number,
                Self::raid_hud_kind_label(state.kind),
                state.seconds_remaining.ceil().max(0.0) as u32
            )
        })
    }

    fn raid_hud_kind_label(kind: UnitKind) -> &'static str {
        match kind {
            UnitKind::Needle => "NEEDLE",
            UnitKind::BellMine => "BELL MINE",
            UnitKind::Canticle => "CANTICLE",
            _ => kind.label(),
        }
    }

    fn order_label(order: UnitOrder) -> &'static str {
        match order {
            UnitOrder::Idle => "READY",
            UnitOrder::Move(_) => "MOVING",
            UnitOrder::AttackMove(_) => "ATTACK-MOVE",
            UnitOrder::Attack(_) => "ENGAGING",
            UnitOrder::Patrol(_, _) => "PATROLLING",
            UnitOrder::Follow(_) => "FOLLOWING",
            UnitOrder::Interact(_) => "OPERATING",
            UnitOrder::Hold => "HOLDING",
        }
    }

    /// Draws the same glyphs twice (a dark offset pass, then the real
    /// color on top) so text reads cleanly against busy backdrop art at the
    /// larger sizes a full-screen menu needs — no font asset required.
    fn draw_text_shadowed(
        &self,
        renderer: &mut Renderer,
        text: &str,
        origin: Vec2,
        pixel: f32,
        color: Color,
        z: f32,
    ) {
        self.draw_text(
            renderer,
            text,
            origin + Vec2::new(pixel * 0.9, -pixel * 0.9),
            pixel,
            Color::rgba(0.0, 0.0, 0.0, 0.75),
            z,
        );
        self.draw_text(renderer, text, origin, pixel, color, z + 0.01);
    }

    /// Fills the whole visible viewport with a dimmed copy of the reactor
    /// sector art (already loaded for gameplay) behind a menu, instead of a
    /// small centered card — reuses `tex_environment`, no new assets.
    fn draw_full_screen_backdrop(&self, ctx: &mut FrameCtx<'_>, tint: Color) {
        let center = ctx.renderer.camera.position;
        let view = ctx.renderer.camera.visible_world_size();
        // Oversize slightly so panning/aspect changes never show a seam.
        let cover = view * 1.05;
        ctx.renderer.draw_sprite(
            self.tex_environment,
            Sprite::new(center, cover)
                .with_color(Color::rgba(0.5, 0.5, 0.55, 1.0))
                .with_z(9.0),
        );
        ctx.renderer.draw_sprite(
            self.tex_ui,
            Sprite::new(center, cover).with_color(tint).with_z(9.5),
        );
    }

    /// Scale the full-screen mission menu against the logical viewport. A
    /// native Retina window can expose fewer world units than the authored
    /// 1280×720 layout, and browser canvases are commonly narrower still.
    /// Keeping a single scale for drawing and hit-testing prevents clipped
    /// headings and “dead” click rows on either platform.
    fn mission_select_scale(view: Vec2) -> f32 {
        (view.x / 1280.0).min(view.y / 720.0).clamp(0.5, 1.0)
    }

    fn mission_menu_cover_size(view: Vec2) -> Vec2 {
        Vec2::new(view.x * 0.38, view.y * 0.38 * 9.0 / 16.0)
    }

    fn mission_entry_rect(camera_position: Vec2, index: usize, scale: f32) -> Aabb {
        // Six authored missions now fit with a deliberate footer gap at the
        // reference 1280x720 viewport. Keeping this in the shared hit-test
        // helper means keyboard, hover, and click selection use the same rows.
        let center = camera_position + Vec2::new(0.0, 130.0 - index as f32 * 58.0) * scale;
        Aabb::from_center_size(center, Vec2::new(780.0, 58.0) * scale)
    }

    fn draw_mission_select(&self, ctx: &mut FrameCtx<'_>) {
        self.draw_full_screen_backdrop(ctx, Color::rgba(0.01, 0.02, 0.045, 0.82));
        let center = ctx.renderer.camera.position;
        let menu_scale = Self::mission_select_scale(ctx.renderer.camera.visible_world_size());
        let cover_size = Self::mission_menu_cover_size(ctx.renderer.camera.visible_world_size());
        let cover_position = center + Vec2::new(-300.0, 155.0) * menu_scale;
        ctx.renderer.draw_sprite(
            self.tex_mission_cover,
            Sprite::new(cover_position, cover_size * menu_scale)
                .with_color(Color::rgba(0.65, 0.65, 0.72, 0.96))
                .with_z(9.2),
        );
        self.draw_text_shadowed(
            ctx.renderer,
            "AURORA: LAST LIGHT",
            center + Vec2::new(-320.0, 330.0) * menu_scale,
            7.5 * menu_scale,
            Color::rgb(0.32, 1.55, 1.35),
            11.0,
        );
        self.draw_text_shadowed(
            ctx.renderer,
            "SELECT MISSION",
            center + Vec2::new(-320.0, 210.0) * menu_scale,
            3.4 * menu_scale,
            Color::rgba(0.7, 0.85, 0.9, 0.95),
            11.0,
        );
        for (index, mission) in missions::all().iter().enumerate() {
            let unlocked = self.save_data.campaign.unlocked_mission >= mission.required_tier;
            let rect = Self::mission_entry_rect(center, index, menu_scale);
            let hovered = index == self.mission_cursor;
            if unlocked {
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(rect.center(), rect.size())
                        .with_color(if hovered {
                            Color::rgba(0.16, 0.55, 0.6, 0.55)
                        } else {
                            Color::rgba(0.05, 0.09, 0.14, 0.55)
                        })
                        .with_z(10.0),
                );
            }
            let color = if !unlocked {
                Color::rgba(0.4, 0.42, 0.46, 0.6)
            } else if hovered {
                Color::rgb(1.3, 0.95, 0.35)
            } else {
                Color::rgb(0.8, 0.9, 0.92)
            };
            let marker = if hovered { ">" } else { " " };
            let label = if unlocked { mission.title } else { "LOCKED" };
            self.draw_text_shadowed(
                ctx.renderer,
                &format!("{marker} {label}"),
                rect.min + Vec2::new(24.0, 22.0) * menu_scale,
                3.4 * menu_scale,
                color,
                11.0,
            );
        }
        self.draw_text_shadowed(
            ctx.renderer,
            "CLICK A MISSION   OR  UP/DOWN + SPACE/ENTER",
            center + Vec2::new(-320.0, -306.0) * menu_scale,
            2.4 * menu_scale,
            Color::rgb(0.6, 0.7, 0.78),
            11.0,
        );
    }

    /// Draws the mission's static obstacles (corridor walls in Mission 3)
    /// as solid panels with a bright edge outline — procedural, matching
    /// the metal/cyan palette already used for structures, no new assets.
    /// These are the same `Aabb`s that block `self.simulation.nav`, so what's drawn
    /// here always matches what the Choir AI (and now the player, via
    /// `route_around_obstacles`) actually treats as solid.
    fn draw_mission_obstacles(&self, renderer: &mut Renderer) {
        for obstacle in &self.mission.obstacles {
            let center = obstacle.center();
            let size = obstacle.size();
            renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(center, size)
                    .with_color(Color::rgba(0.09, 0.12, 0.16, 0.96))
                    .with_z(-8.0),
            );
            let edge_color = Color::rgba(0.25, 0.55, 0.65, 0.85);
            let half = size * 0.5;
            for (offset, dimensions) in [
                (Vec2::new(0.0, half.y), Vec2::new(size.x, 3.0)),
                (Vec2::new(0.0, -half.y), Vec2::new(size.x, 3.0)),
                (Vec2::new(half.x, 0.0), Vec2::new(3.0, size.y)),
                (Vec2::new(-half.x, 0.0), Vec2::new(3.0, size.y)),
            ] {
                renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(center + offset, dimensions)
                        .with_color(edge_color)
                        .with_z(-7.9),
                );
            }
        }
    }

    /// Renders authored elevation/cover bands as restrained terrain overlays.
    /// The simulation already consumes these zones for combat multipliers;
    /// drawing them here makes the strategic reason to fight for a ridge or
    /// covered pocket legible instead of leaving the bonus invisible.
    fn draw_terrain_zones(&self, renderer: &mut Renderer) {
        let onboarding = self.controls_hint_remaining > 0.0;
        for zone in &self.mission.terrain_zones {
            let bounds = zone.bounds;
            let center = bounds.center();
            let size = bounds.size();
            let elevation = zone.elevation.max(0) as f32;
            let cover = zone.cover.clamp(0.0, 0.3);
            let fill = if zone.elevation > 0 {
                Color::rgba(
                    0.08,
                    0.48,
                    0.58,
                    if onboarding {
                        0.055 + cover * 0.12
                    } else {
                        0.012 + cover * 0.035
                    },
                )
            } else {
                Color::rgba(
                    0.36,
                    0.22,
                    0.48,
                    if onboarding {
                        0.04 + cover * 0.1
                    } else {
                        0.01 + cover * 0.028
                    },
                )
            };
            renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(center, size).with_color(fill).with_z(-7.7),
            );
            let edge = if zone.elevation > 0 {
                // Keep the band readable without competing with units, alerts,
                // and the command card when a large ridge crosses the camera.
                Color::rgba(
                    0.2,
                    0.9,
                    1.0,
                    if onboarding {
                        0.15 + cover * 0.25
                    } else {
                        0.075 + cover * 0.1
                    },
                )
            } else {
                Color::rgba(
                    0.65,
                    0.45,
                    0.9,
                    if onboarding {
                        0.10 + cover * 0.18
                    } else {
                        0.06 + cover * 0.08
                    },
                )
            };
            let half = size * 0.5;
            for (offset, dimensions) in [
                (Vec2::new(0.0, half.y), Vec2::new(size.x, 1.5)),
                (Vec2::new(0.0, -half.y), Vec2::new(size.x, 1.5)),
                (Vec2::new(half.x, 0.0), Vec2::new(1.5, size.y)),
                (Vec2::new(-half.x, 0.0), Vec2::new(1.5, size.y)),
            ] {
                renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(center + offset, dimensions)
                        .with_color(edge)
                        .with_z(-7.6),
                );
            }
            if elevation > 0.0 || cover > 0.0 {
                let hatch_count = ((size.x / 120.0).floor() as usize).clamp(2, 8);
                for index in 0..hatch_count {
                    let x = bounds.min.x + (index as f32 + 0.5) * size.x / hatch_count as f32;
                    renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(
                            Vec2::new(x, bounds.max.y - 12.0),
                            Vec2::new(26.0, 2.0 + elevation * 1.5),
                        )
                        .with_rotation(-0.45)
                        .with_color(edge)
                        .with_z(-7.55),
                    );
                }
            }
        }
    }

    fn draw_selection_brackets(&self, renderer: &mut Renderer, position: Vec2, size: f32) {
        let color = Color::rgba(0.22, 1.8, 1.45, 0.95);
        for (offset, dimensions) in [
            (Vec2::new(0.0, size), Vec2::new(size * 1.5, 3.0)),
            (Vec2::new(0.0, -size), Vec2::new(size * 1.5, 3.0)),
            (Vec2::new(size, 0.0), Vec2::new(3.0, size * 1.5)),
            (Vec2::new(-size, 0.0), Vec2::new(3.0, size * 1.5)),
        ] {
            renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(position + offset, dimensions)
                    .with_color(color)
                    .with_z(2.2),
            );
        }
    }
}

impl Game for LastLight {
    fn name(&self) -> &str {
        "Aurora: Last Light — Reclaim the Reactor"
    }

    fn on_start(&mut self, renderer: &mut Renderer) {
        debug_assert_eq!(assets::manifest().len(), TextureAsset::ALL.len());
        let (
            environment,
            units,
            warden_move,
            warden_attack,
            engineer_move,
            engineer_repair,
            engineer_build,
            surveyor_move,
            surveyor_scan,
            surveyor_mark,
            needle_attack,
            canticle_command,
            bell_mine_arm,
            hit_reactions,
            down_reactions,
            structures,
            portraits,
            resources,
            resource_effects,
            glow,
            ui,
            mission_cover,
        ) = {
            let gpu = renderer.gpu();
            (
                assets::load_texture(&gpu, TextureAsset::ReactorSector),
                assets::load_texture(&gpu, TextureAsset::Units),
                assets::load_texture(&gpu, TextureAsset::WardenMove),
                assets::load_texture(&gpu, TextureAsset::WardenAttack),
                assets::load_texture(&gpu, TextureAsset::EngineerMove),
                assets::load_texture(&gpu, TextureAsset::EngineerRepair),
                assets::load_texture(&gpu, TextureAsset::EngineerBuild),
                assets::load_texture(&gpu, TextureAsset::SurveyorMove),
                assets::load_texture(&gpu, TextureAsset::SurveyorScan),
                assets::load_texture(&gpu, TextureAsset::SurveyorMark),
                assets::load_texture(&gpu, TextureAsset::NeedleAttack),
                assets::load_texture(&gpu, TextureAsset::CanticleCommand),
                assets::load_texture(&gpu, TextureAsset::BellMineArm),
                assets::load_texture(&gpu, TextureAsset::HitReactions),
                assets::load_texture(&gpu, TextureAsset::DownReactions),
                assets::load_texture(&gpu, TextureAsset::Structures),
                assets::load_texture(&gpu, TextureAsset::CommandPortraits),
                assets::load_texture(&gpu, TextureAsset::ResourceNodes),
                assets::load_texture(&gpu, TextureAsset::ResourceHarvestEffects),
                Texture::soft_circle(&gpu, 64, Color::WHITE),
                Texture::solid(&gpu, Color::WHITE),
                Texture::from_bytes(
                    &gpu,
                    include_bytes!("../assets/cover/aurora-last-light-cover-v001.png"),
                    "menu.cover.aurora-last-light",
                )
                .expect("cover texture should decode"),
            )
        };
        self.tex_environment = renderer.add_texture(environment);
        self.tex_units = renderer.add_texture(units);
        self.tex_warden_move = renderer.add_texture(warden_move);
        self.tex_warden_attack = renderer.add_texture(warden_attack);
        self.tex_engineer_move = renderer.add_texture(engineer_move);
        self.tex_engineer_repair = renderer.add_texture(engineer_repair);
        self.tex_engineer_build = renderer.add_texture(engineer_build);
        self.tex_surveyor_move = renderer.add_texture(surveyor_move);
        self.tex_surveyor_scan = renderer.add_texture(surveyor_scan);
        self.tex_surveyor_mark = renderer.add_texture(surveyor_mark);
        self.tex_needle_attack = renderer.add_texture(needle_attack);
        self.tex_canticle_command = renderer.add_texture(canticle_command);
        self.tex_bell_mine_arm = renderer.add_texture(bell_mine_arm);
        self.tex_hit_reactions = renderer.add_texture(hit_reactions);
        self.tex_down_reactions = renderer.add_texture(down_reactions);
        self.tex_structures = renderer.add_texture(structures);
        self.tex_portraits = renderer.add_texture(portraits);
        self.tex_mission_cover = renderer.add_texture(mission_cover);
        self.tex_resources = renderer.add_texture(resources);
        self.tex_resource_effects = renderer.add_texture(resource_effects);
        self.tex_glow = renderer.add_texture(glow);
        self.tex_ui = renderer.add_texture(ui);
        self.unit_atlas = TextureAsset::Units.runtime_atlas(self.tex_units);
        self.warden_move_atlas = TextureAsset::WardenMove.runtime_atlas(self.tex_warden_move);
        self.warden_attack_atlas = TextureAsset::WardenAttack.runtime_atlas(self.tex_warden_attack);
        self.engineer_move_atlas = TextureAsset::EngineerMove.runtime_atlas(self.tex_engineer_move);
        self.engineer_repair_atlas =
            TextureAsset::EngineerRepair.runtime_atlas(self.tex_engineer_repair);
        self.engineer_build_atlas =
            TextureAsset::EngineerBuild.runtime_atlas(self.tex_engineer_build);
        self.surveyor_move_atlas = TextureAsset::SurveyorMove.runtime_atlas(self.tex_surveyor_move);
        self.surveyor_scan_atlas = TextureAsset::SurveyorScan.runtime_atlas(self.tex_surveyor_scan);
        self.surveyor_mark_atlas = TextureAsset::SurveyorMark.runtime_atlas(self.tex_surveyor_mark);
        self.needle_attack_atlas = TextureAsset::NeedleAttack.runtime_atlas(self.tex_needle_attack);
        self.canticle_command_atlas =
            TextureAsset::CanticleCommand.runtime_atlas(self.tex_canticle_command);
        self.bell_mine_arm_atlas = TextureAsset::BellMineArm.runtime_atlas(self.tex_bell_mine_arm);
        self.hit_reactions_atlas = TextureAsset::HitReactions.runtime_atlas(self.tex_hit_reactions);
        self.down_reactions_atlas =
            TextureAsset::DownReactions.runtime_atlas(self.tex_down_reactions);
        self.structure_atlas = TextureAsset::Structures.runtime_atlas(self.tex_structures);
        self.portrait_atlas = TextureAsset::CommandPortraits.runtime_atlas(self.tex_portraits);
        self.resource_atlas = TextureAsset::ResourceNodes.runtime_atlas(self.tex_resources);
        self.resource_effects_atlas =
            TextureAsset::ResourceHarvestEffects.runtime_atlas(self.tex_resource_effects);
        renderer.camera.position = Vec2::new(-700.0, -260.0);
        renderer.camera.zoom = 1.1;
        renderer.camera.zoom_min = 0.9;
        renderer.camera.zoom_max = 1.75;
        renderer.post_fx.bloom_intensity = 0.78;
        renderer.post_fx.vignette = 0.48;
        renderer.post_fx.chromatic = 0.0015;
        renderer.set_clear_color(Color::rgb(0.007, 0.014, 0.025));
    }

    fn on_fixed_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let dt = ctx.time.fixed_dt;
        self.update_camera(ctx, dt);
        // Only the first fixed-step of this rendered frame should react to
        // edge-triggered input (see field doc on `input_handled_this_frame`).
        // Continuous simulation below still runs every fixed step so the
        // catch-up loop can still catch up after a hitch.
        let handle_input = !self.input_handled_this_frame;
        self.input_handled_this_frame = true;

        if self.mission_select {
            if handle_input {
                self.handle_mission_select(ctx);
            }
            return;
        }
        if self.briefing {
            if handle_input {
                self.handle_briefing_upgrades(ctx);
                if ctx.input.key_pressed(KeyCode::Space) || ctx.input.key_pressed(KeyCode::Enter) {
                    self.briefing = false;
                    self.focus_player_roster(ctx);
                    ctx.audio.start();
                }
            }
            return;
        }
        if handle_input {
            if ctx.input.key_pressed(KeyCode::Escape) {
                if self.placing_beacon {
                    self.placing_beacon = false;
                    self.status = Some(("BEACON PLACEMENT CANCELLED".to_owned(), 2.0));
                } else {
                    self.paused = !self.paused;
                }
            }
            if ctx.input.mouse_pressed(MouseButton::Left)
                && !self.placing_beacon
                && Self::pause_icon_rect(ctx.renderer).contains_point(
                    ctx.renderer
                        .camera
                        .screen_to_world(ctx.input.mouse_position),
                )
            {
                self.paused = !self.paused;
            }
        }
        if self.victory || self.defeat {
            if handle_input {
                for key in [
                    KeyCode::Space,
                    KeyCode::Enter,
                    KeyCode::NumpadEnter,
                    KeyCode::Escape,
                ] {
                    if ctx.input.key_pressed(key) {
                        self.handle_terminal_input(key);
                        break;
                    }
                }
            }
            return;
        }
        if self.paused {
            return;
        }

        if handle_input {
            self.handle_command_keys(ctx);
            self.handle_pointer(ctx);
        }
        self.update_enemy_ai(dt);
        self.update_auto_targeting();
        let modifiers = self.simulation_modifiers();
        self.simulation.set_combat_scales(
            modifiers.player_damage_scale,
            modifiers.player_damage_taken_scale,
        );
        self.simulation.fixed_step_with_dt(dt);
        self.process_simulation_events(ctx);
        for unit in self.simulation.world.units() {
            let Some(kind) = self.simulation.kinds.get(&unit.id).copied() else {
                continue;
            };
            let engaged = unit.alive() && self.unit_engaged(unit.id);
            let state =
                self.unit_animation_state(unit.id, kind, unit.alive(), unit.velocity, engaged);
            let clip = self.unit_animation_clip(kind, state);
            let Some(player) = self.animation_players.get_mut(&unit.id) else {
                continue;
            };
            if let Some(clip) = clip {
                player.play(clip);
                player.tick(dt);
            } else {
                player.clear();
            }
        }
        self.update_combat_presentation(dt);
        self.update_specialist_doctrines(dt);
        self.update_fog();
        self.update_status_timer(dt);
        if self.update_harvesting(dt) > 0 {
            self.status = Some(("SURVEYOR HARVESTING SALVAGE".to_owned(), 0.8));
        }
        self.update_specialist_objective(dt);
        self.update_terrain_control_objective(dt);
        self.mission_time += dt;
        self.update_radio_dialogue(dt);
        if let Some((_, time)) = self.order_marker.as_mut() {
            *time -= dt;
            if *time <= 0.0 {
                self.order_marker = None;
            }
        }

        if let Some(console) = self.mission.lumen_console {
            if !self.save_data.campaign.has_decision(LUMEN_AWAKENED)
                && ctx.input.key_pressed(KeyCode::KeyK)
                && self.selected_engineer_near(console)
            {
                self.awaken_lumen_console();
            }
        }

        self.evaluate_mission_state();
        if self.victory {
            self.persist_victory();
        }
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        // on_update runs exactly once per rendered frame (unlike
        // on_fixed_update, which can run several times after a hitch), so
        // this is the correct place to end the suppression window opened by
        // a menu transition earlier in this same frame.
        self.input_handled_this_frame = false;
        self.normalize_selection_context();
        self.clamp_command_card_page_to_context();
        let t = ctx.time.elapsed;
        // Comms are the highest-priority onboarding surface. While a person
        // is speaking, suppress the lower-priority controls legend so the
        // portrait and line read as one intentional transmission instead of
        // stacking tutorial copy over the playfield.
        let controls_hint_visible = self.controls_hint_visible();
        if self.mission_select {
            self.draw_mission_select(ctx);
            return;
        }
        ctx.renderer.draw_sprite(
            self.tex_environment,
            Sprite::new(Vec2::ZERO, Self::environment_sprite_size()).with_z(-10.0),
        );
        self.draw_terrain_zones(ctx.renderer);
        self.draw_mission_obstacles(ctx.renderer);

        for y in 0..15 {
            for x in 0..26 {
                let center =
                    -MAP_SIZE * 0.5 + Vec2::new(x as f32 * 100.0 + 50.0, y as f32 * 100.0 + 50.0);
                let alpha = match self.fog.state_at(center) {
                    FogState::Visible => continue,
                    FogState::Explored => 0.42,
                    FogState::Hidden => 0.88,
                };
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(center, Vec2::splat(102.0))
                        .with_color(Color::rgba(0.005, 0.008, 0.018, alpha))
                        .with_z(-3.0),
                );
            }
        }

        if let Some(reactor_position) = self.reactor_position {
            let reactor_pulse = 0.55 + 0.12 * (t * 2.1).sin();
            let mut reactor = self
                .structure_atlas
                .sprite(reactor_position, Vec2::splat(330.0), 2);
            reactor.z = -1.0;
            ctx.renderer.draw_sprite(self.tex_structures, reactor);
            self.draw_structure_state_fx(
                ctx.renderer,
                StructureKind::Reactor,
                reactor_position,
                Vec2::splat(330.0),
                t,
            );
            ctx.renderer.draw_light(PointLight::new(
                reactor_position,
                Color::rgb(0.16, 0.58, 0.8),
                260.0,
                reactor_pulse * 0.26,
            ));
        }

        for (index, relay) in self.simulation.relays.iter().enumerate() {
            if !relay.active
                || !self
                    .simulation
                    .power
                    .is_powered(PowerNodeId(index as u16 + 1))
            {
                continue;
            }
            let offset = relay.position - self.fabricator_position;
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    self.fabricator_position + offset * 0.5,
                    Vec2::new(offset.length(), 4.0),
                )
                .with_color(Color::rgba(0.08, 1.35, 1.1, 0.34))
                .with_rotation(offset.y.atan2(offset.x))
                .with_z(-1.6),
            );
        }
        let mut fabricator =
            self.structure_atlas
                .sprite(self.fabricator_position, Vec2::splat(190.0), 1);
        fabricator.z = -0.4;
        ctx.renderer.draw_sprite(self.tex_structures, fabricator);
        ctx.renderer.draw_light(PointLight::new(
            self.fabricator_position,
            Color::rgb(0.16, 1.0, 0.85),
            190.0,
            0.22,
        ));
        self.draw_structure_state_fx(
            ctx.renderer,
            StructureKind::Fabricator,
            self.fabricator_position,
            Vec2::splat(190.0),
            t,
        );

        for (node_index, node) in self.salvage_nodes.iter().enumerate() {
            let charge = node.remaining as f32 / 240.0;
            let worker_count = self.workers_at_node(node_index);
            if charge <= 0.0 {
                if self.selected_resource_node == Some(node_index) || worker_count > 0 {
                    let mut depleted_effect =
                        self.resource_effects_atlas
                            .sprite(node.position, Vec2::splat(176.0), 3);
                    depleted_effect.color = Color::rgba(0.72, 0.9, 0.92, 0.48);
                    depleted_effect.z = -0.02;
                    ctx.renderer
                        .draw_sprite(self.tex_resource_effects, depleted_effect);
                    self.draw_text(
                        ctx.renderer,
                        &format!("NODE {}  DRY", node_index + 1),
                        node.position + Vec2::new(-52.0, -92.0),
                        1.25,
                        Color::rgba(0.72, 0.78, 0.82, 0.8),
                        1.5,
                    );
                }
                continue;
            }
            let pulse = 0.82 + (t * 3.2 + node.position.x * 0.01).sin() * 0.12;
            let node_color = match node.kind {
                ResourceKind::Salvage => Color::rgba(0.08, 1.4, 1.35, 0.085 * pulse),
                ResourceKind::Flux => Color::rgba(0.72, 0.2, 1.55, 0.11 * pulse),
            };
            ctx.renderer.draw_sprite(
                self.tex_glow,
                Sprite::new(node.position, Vec2::splat(108.0 * charge.max(0.55)))
                    .with_color(node_color)
                    .with_z(-0.25),
            );
            let frame = match (node.kind, worker_count > 0) {
                (ResourceKind::Salvage, false) => 0,
                (ResourceKind::Flux, false) => 1,
                (ResourceKind::Salvage, true) => 2,
                (ResourceKind::Flux, true) => 3,
            };
            let mut resource_sprite = self.resource_atlas.sprite(
                node.position,
                Vec2::splat(208.0 * (0.86 + charge * 0.14)),
                frame,
            );
            resource_sprite.color = Color::rgba(1.0, 1.0, 1.0, 0.94);
            resource_sprite.z = -0.05;
            ctx.renderer
                .draw_sprite(self.tex_resources, resource_sprite);
            if worker_count > 0 {
                let extracting = self.harvest_jobs.values().any(|job| {
                    job.node == node_index && matches!(job.phase, HarvestPhase::Extracting)
                });
                let hauling = self.harvest_jobs.iter().find_map(|(id, job)| {
                    (job.node == node_index
                        && matches!(job.phase, HarvestPhase::ToDepot)
                        && job.cargo > 0)
                        .then(|| self.simulation.world.unit(*id).map(|unit| unit.position))
                        .flatten()
                });
                let (effect_frame, effect_position, effect_size) = if let Some(position) = hauling {
                    (2, position + Vec2::new(0.0, 26.0), 122.0)
                } else if extracting {
                    (
                        if node.kind == ResourceKind::Flux {
                            1
                        } else {
                            0
                        },
                        node.position,
                        184.0,
                    )
                } else {
                    (3, node.position, 150.0)
                };
                let mut harvest_effect = self.resource_effects_atlas.sprite(
                    effect_position,
                    Vec2::splat(effect_size),
                    effect_frame,
                );
                harvest_effect.color = Color::rgba(1.0, 1.0, 1.0, 0.72 + pulse * 0.12);
                harvest_effect.z = 0.06;
                ctx.renderer
                    .draw_sprite(self.tex_resource_effects, harvest_effect);
            }
            if worker_count > 0 {
                self.draw_text(
                    ctx.renderer,
                    &format!("WORK {} / {}", worker_count, node.max_workers),
                    node.position + Vec2::new(-42.0, -92.0),
                    1.25,
                    if worker_count >= node.max_workers as usize {
                        Color::rgba(1.15, 0.72, 0.28, 0.92)
                    } else {
                        Color::rgba(0.68, 0.92, 0.88, 0.86)
                    },
                    1.5,
                );
            }
        }

        for (index, relay) in self.simulation.relays.iter().enumerate() {
            let progress = if relay.active {
                1.0
            } else {
                relay.progress / 3.0
            };
            let mut sprite = self
                .structure_atlas
                .sprite(relay.position, Vec2::splat(160.0), 0);
            sprite.color = if relay.active {
                Color::WHITE
            } else {
                Color::rgba(
                    0.34 + progress * 0.66,
                    0.38 + progress * 0.62,
                    0.42 + progress * 0.58,
                    1.0,
                )
            };
            sprite.z = -0.5;
            ctx.renderer.draw_sprite(self.tex_structures, sprite);
            ctx.renderer.draw_light(PointLight::new(
                relay.position,
                Color::rgb(0.12, 1.1, 1.0),
                150.0,
                0.06 + progress * 0.28,
            ));
            self.draw_structure_state_fx(
                ctx.renderer,
                StructureKind::Relay(index),
                relay.position,
                Vec2::splat(160.0),
                t,
            );
        }

        for beacon in &self.field_beacons {
            let mut sprite = self
                .structure_atlas
                .sprite(beacon.position, Vec2::splat(96.0), 0);
            sprite.z = -0.35;
            ctx.renderer.draw_sprite(self.tex_structures, sprite);
            ctx.renderer.draw_light(PointLight::new(
                beacon.position,
                Color::rgb(0.1, 1.25, 1.0),
                210.0,
                0.22 + (t * 3.0).sin().abs() * 0.08,
            ));
        }

        if let Some((position, remaining)) = self.simulation.scan_pulse {
            let pulse = (5.0 - remaining).clamp(0.0, 5.0) / 5.0;
            let radius = 140.0 + pulse * 620.0;
            ctx.renderer.draw_sprite(
                self.tex_glow,
                Sprite::new(position, Vec2::splat(radius * 2.0))
                    .with_color(Color::rgba(0.3, 0.82, 1.35, 0.12 * (1.0 - pulse * 0.35)))
                    .with_z(1.4),
            );
            self.draw_selection_brackets(ctx.renderer, position, radius);
        }

        if self.placing_beacon {
            let position = ctx
                .renderer
                .camera
                .screen_to_world(ctx.input.mouse_position);
            let rules = self.placement_rules();
            for source in &rules.power_sources {
                ctx.renderer.draw_sprite(
                    self.tex_glow,
                    Sprite::new(*source, Vec2::splat(rules.max_power_distance * 2.0))
                        .with_color(Color::rgba(0.04, 0.7, 0.62, 0.065))
                        .with_z(3.6),
                );
                self.draw_selection_brackets(ctx.renderer, *source, 62.0);
            }
            let valid = rules.validate(position, 54.0).is_ok()
                && self.simulation.resources.amount() >= self.beacon_cost();
            let mut preview = self.structure_atlas.sprite(position, Vec2::splat(108.0), 0);
            preview.color = if valid {
                Color::rgba(0.28, 1.25, 1.05, 0.64)
            } else {
                Color::rgba(1.45, 0.16, 0.3, 0.62)
            };
            preview.z = 4.0;
            ctx.renderer.draw_sprite(self.tex_structures, preview);
            ctx.renderer.draw_sprite(
                self.tex_glow,
                Sprite::new(position, Vec2::splat(940.0))
                    .with_color(if valid {
                        Color::rgba(0.08, 1.0, 0.8, 0.055)
                    } else {
                        Color::rgba(1.2, 0.05, 0.15, 0.045)
                    })
                    .with_z(3.8),
            );
        }

        for unit in self.simulation.world.units() {
            if unit.faction == CHOIR {
                let fog_state = self.fog.state_at(unit.position);
                if (unit.alive() && fog_state != FogState::Visible)
                    || (!unit.alive() && fog_state == FogState::Hidden)
                {
                    continue;
                }
            }
            let kind = self.simulation.kinds[&unit.id];
            let engaged = unit.alive() && self.unit_engaged(unit.id);
            let animation_state =
                self.unit_animation_state(unit.id, kind, unit.alive(), unit.velocity, engaged);
            let frame = self
                .animation_players
                .get(&unit.id)
                .map(AnimationPlayer::frame)
                .unwrap_or(kind.atlas_frame() * 4);
            if !unit.alive() {
                let mut wreck = self.down_reactions_atlas.sprite(
                    unit.position,
                    Vec2::splat(kind.scale()),
                    frame,
                );
                wreck.color = Color::rgba(0.78, 0.82, 0.88, 0.94);
                wreck.z = 0.55;
                ctx.renderer.draw_sprite(self.tex_down_reactions, wreck);
                continue;
            }
            let selected = self.simulation.world.selection().contains(unit.id);
            if selected {
                self.draw_selection_brackets(ctx.renderer, unit.position, unit.radius * 1.35);
            }
            let glow_color = if unit.faction == PLAYER {
                Color::rgba(0.1, 1.45, 1.25, if selected { 0.28 } else { 0.10 })
            } else {
                Color::rgba(1.7, 0.08, 0.58, 0.18)
            };
            ctx.renderer.draw_sprite(
                self.tex_glow,
                Sprite::new(unit.position, Vec2::splat(kind.scale() * 1.8))
                    .with_color(glow_color)
                    .with_z(-0.2),
            );
            let animated = self.unit_animation_atlas(kind, animation_state);
            let (texture, mut sprite) = animated.map_or_else(
                || {
                    (
                        self.tex_units,
                        self.unit_atlas.sprite(
                            unit.position,
                            Vec2::splat(kind.scale()),
                            kind.atlas_frame(),
                        ),
                    )
                },
                |(texture, atlas)| {
                    (
                        texture,
                        atlas.sprite(unit.position, Vec2::splat(kind.scale()), frame),
                    )
                },
            );
            // Work/attack poses are authored upright. Only the movement strip
            // (or a quiet idle frame with no strip) follows travel direction.
            if matches!(
                animation_state,
                UnitAnimationState::Move | UnitAnimationState::Idle
            ) && unit.velocity.length_squared() > 1.0
            {
                sprite.rotation = unit_sprite_rotation(unit.velocity);
            }
            sprite.z = 1.0;
            ctx.renderer.draw_sprite(texture, sprite);

            match animation_state {
                UnitAnimationState::Build => {
                    let pulse = (self.mission_time * 8.0 + unit.id.0 as f32 * 0.31)
                        .sin()
                        .abs();
                    ctx.renderer.draw_sprite(
                        self.tex_glow,
                        Sprite::new(
                            unit.position + Vec2::new(0.0, unit.radius * 0.42),
                            Vec2::splat(unit.radius * (1.45 + pulse * 0.3)),
                        )
                        .with_color(Color::rgba(1.35, 0.72, 0.16, 0.22 + pulse * 0.18))
                        .with_z(2.08),
                    );
                    self.draw_selection_brackets(ctx.renderer, unit.position, unit.radius * 1.7);
                }
                UnitAnimationState::Mark => {
                    let pulse = (self.mission_time * 10.0 + unit.id.0 as f32 * 0.23)
                        .sin()
                        .abs();
                    ctx.renderer.draw_sprite(
                        self.tex_glow,
                        Sprite::new(
                            unit.position + Vec2::new(0.0, -unit.radius * 0.38),
                            Vec2::splat(unit.radius * (1.25 + pulse * 0.35)),
                        )
                        .with_color(Color::rgba(0.18, 1.32, 1.45, 0.2 + pulse * 0.2))
                        .with_z(2.08),
                    );
                    self.draw_selection_brackets(ctx.renderer, unit.position, unit.radius * 1.55);
                }
                _ => {}
            }

            if self.attack_flash.contains_key(&unit.id) {
                if let Some(target_id) = self.attack_target_for_unit(unit.id) {
                    if let Some(target) = self.simulation.world.unit(target_id) {
                        let delta = target.position - unit.position;
                        let beam_color = if unit.faction == PLAYER {
                            Color::rgba(0.18, 1.7, 1.35, 0.92)
                        } else {
                            Color::rgba(1.8, 0.12, 0.62, 0.92)
                        };
                        ctx.renderer.draw_sprite(
                            self.tex_ui,
                            Sprite::new(
                                unit.position + delta * 0.5,
                                Vec2::new(delta.length(), 4.0),
                            )
                            .with_color(beam_color)
                            .with_rotation(delta.y.atan2(delta.x))
                            .with_z(2.2),
                        );
                        ctx.renderer.draw_sprite(
                            self.tex_glow,
                            Sprite::new(target.position, Vec2::splat(42.0))
                                .with_color(beam_color)
                                .with_z(2.15),
                        );
                        // Keep a deterministic source pulse for roles that
                        // still use a procedural attack cue; authored strips
                        // (including the Warden fire cycle) skip this layer.
                        if self.unit_animation_atlas(kind, animation_state).is_none() {
                            let direction = delta.normalize_or_zero();
                            let source = unit.position + direction * (kind.scale() * 0.28);
                            let pulse = (self.mission_time * 42.0 + unit.id.0 as f32 * 0.73)
                                .sin()
                                .abs();
                            ctx.renderer.draw_sprite(
                                self.tex_glow,
                                Sprite::new(source, Vec2::splat(18.0 + pulse * 14.0))
                                    .with_color(Color::rgba(
                                        beam_color.r,
                                        beam_color.g,
                                        beam_color.b,
                                        0.55 + pulse * 0.35,
                                    ))
                                    .with_z(2.21),
                            );
                        }
                    }
                }
            }

            // A visible enemy attack is telegraphed before the hit lands. The
            // target comes from the same order the combat resolver consumes,
            // so this cannot point at a stale or unrelated contact.
            if unit.faction == CHOIR {
                if let UnitOrder::Attack(target_id) = unit.order {
                    if let Some(target) = self.simulation.world.unit(target_id) {
                        let pulse = (t * 7.0 + unit.id.0 as f32 * 0.37).sin().abs();
                        let (size, alpha) = match kind {
                            UnitKind::BellMine => (132.0, 0.22),
                            UnitKind::Canticle => (154.0, 0.16),
                            UnitKind::Needle => (96.0, 0.11),
                            _ => (88.0, 0.08),
                        };
                        ctx.renderer.draw_sprite(
                            self.tex_glow,
                            Sprite::new(target.position, Vec2::splat(size + pulse * 18.0))
                                .with_color(Color::rgba(1.65, 0.04, 0.5, alpha + pulse * 0.08))
                                .with_z(2.05),
                        );
                        let delta = target.position - unit.position;
                        ctx.renderer.draw_sprite(
                            self.tex_ui,
                            Sprite::new(
                                unit.position + delta * 0.5,
                                Vec2::new(delta.length(), 2.0),
                            )
                            .with_color(Color::rgba(1.5, 0.08, 0.48, 0.28 + pulse * 0.22))
                            .with_rotation(delta.y.atan2(delta.x))
                            .with_z(2.04),
                        );
                    }
                }
            }

            if let Some((target_id, _)) = self.repair_flash.get(&unit.id) {
                if let Some(target) = self.simulation.world.unit(*target_id) {
                    let delta = target.position - unit.position;
                    let repair_color = Color::rgba(1.55, 0.82, 0.18, 0.92);
                    ctx.renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(unit.position + delta * 0.5, Vec2::new(delta.length(), 3.0))
                            .with_color(repair_color)
                            .with_rotation(delta.y.atan2(delta.x))
                            .with_z(2.25),
                    );
                    ctx.renderer.draw_sprite(
                        self.tex_glow,
                        Sprite::new(target.position, Vec2::splat(34.0))
                            .with_color(repair_color)
                            .with_z(2.2),
                    );
                }
            }

            if let Some(job) = self.harvest_jobs.get(&unit.id).filter(|job| job.cargo > 0) {
                let fill = job.cargo as f32 / 24.0;
                ctx.renderer.draw_sprite(
                    self.tex_glow,
                    Sprite::new(unit.position + Vec2::new(0.0, 34.0), Vec2::splat(28.0))
                        .with_color(Color::rgba(1.45, 0.74, 0.16, 0.35 + fill * 0.45))
                        .with_z(2.25),
                );
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(
                        unit.position + Vec2::new(-12.0 + fill * 12.0, 34.0),
                        Vec2::new(24.0 * fill, 5.0),
                    )
                    .with_color(Color::rgba(1.5, 0.82, 0.22, 0.96))
                    .with_z(2.35),
                );
            }

            let health = (unit.health / unit.max_health).clamp(0.0, 1.0);
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    unit.position + Vec2::new(0.0, -unit.radius * 1.6),
                    Vec2::new(70.0, 5.0),
                )
                .with_color(Color::rgba(0.02, 0.03, 0.04, 0.9))
                .with_z(2.3),
            );
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    unit.position + Vec2::new(-35.0 + health * 35.0, -unit.radius * 1.6),
                    Vec2::new(70.0 * health, 3.0),
                )
                .with_color(if unit.faction == PLAYER {
                    Color::rgba(0.2, 1.5, 1.15, 1.0)
                } else {
                    Color::rgba(1.7, 0.15, 0.5, 1.0)
                })
                .with_z(2.4),
            );
        }

        if let Some(structure) = self.selected_structure {
            if let Some(position) = self.structure_position(structure) {
                let radius = match structure {
                    StructureKind::Relay(_) => StructureKind::RELAY_RADIUS,
                    StructureKind::Fabricator => StructureKind::FABRICATOR_RADIUS,
                    StructureKind::Reactor => StructureKind::REACTOR_RADIUS,
                };
                self.draw_selection_brackets(ctx.renderer, position, radius);
            } else {
                self.selected_structure = None;
                self.reset_command_card_page();
            }
        }

        if let Some(node_index) = self.selected_resource_node {
            if let Some(node) = self.salvage_nodes.get(node_index) {
                self.draw_selection_brackets(ctx.renderer, node.position, 82.0);
            } else {
                self.selected_resource_node = None;
                self.reset_command_card_page();
            }
        }

        if let Some(drag) = self.drag {
            let bounds = drag.bounds();
            let center = bounds.center();
            let size = bounds.size();
            let color = Color::rgba(0.15, 1.6, 1.35, 0.9);
            for (position, dimensions) in [
                (Vec2::new(center.x, bounds.min.y), Vec2::new(size.x, 3.0)),
                (Vec2::new(center.x, bounds.max.y), Vec2::new(size.x, 3.0)),
                (Vec2::new(bounds.min.x, center.y), Vec2::new(3.0, size.y)),
                (Vec2::new(bounds.max.x, center.y), Vec2::new(3.0, size.y)),
            ] {
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(position, dimensions)
                        .with_color(color)
                        .with_z(5.0),
                );
            }
        }

        if let Some((position, time)) = self.order_marker {
            let size = 18.0 + time * 42.0;
            self.draw_selection_brackets(ctx.renderer, position, size);
        }

        if let Some(rally) = self.simulation.rally_point {
            let delta = rally - self.fabricator_position;
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    self.fabricator_position + delta * 0.5,
                    Vec2::new(delta.length(), 2.0),
                )
                .with_color(Color::rgba(0.2, 1.25, 1.1, 0.55))
                .with_rotation(delta.y.atan2(delta.x))
                .with_z(1.8),
            );
            self.draw_selection_brackets(ctx.renderer, rally, 34.0);
        }

        if !self.briefing && !self.victory && !self.defeat {
            if let Some((position, _)) = self.next_objective() {
                let pulse = 76.0 + (t * 3.4).sin().abs() * 24.0;
                ctx.renderer.draw_sprite(
                    self.tex_glow,
                    Sprite::new(position, Vec2::splat(pulse * 2.4))
                        .with_color(Color::rgba(0.95, 0.66, 0.16, 0.18))
                        .with_z(2.0),
                );
                self.draw_selection_brackets(ctx.renderer, position, pulse);
            }
        }

        // A short world-space callout makes a radio line actionable: the
        // player can see which relay/array the speaker means, then the label
        // fades away so the battlefield remains the dominant visual. This is
        // deliberately separate from the persistent objective beacon and the
        // Space-to-focus inbox behavior.
        if !self.briefing && !self.victory && !self.defeat {
            if let Some((position, label, remaining)) = self.target_feedback.as_ref() {
                let fade = (remaining / 3.5).clamp(0.0, 1.0);
                let pulse = 42.0 + (t * 6.0).sin().abs() * 12.0;
                let accent = if label.starts_with("COMMS") {
                    Color::rgba(0.22, 1.35, 1.2, 0.78 * fade)
                } else {
                    Color::rgba(1.15, 0.72, 0.22, 0.82 * fade)
                };
                ctx.renderer.draw_sprite(
                    self.tex_glow,
                    Sprite::new(*position, Vec2::splat(pulse * 1.45))
                        .with_color(Color::rgba(accent.r, accent.g, accent.b, 0.14 * fade))
                        .with_z(2.05),
                );
                self.draw_selection_brackets(ctx.renderer, *position, pulse);
                if self.radio_message.is_none() {
                    self.draw_text_shadowed(
                        ctx.renderer,
                        label,
                        *position + Vec2::new(-120.0, -78.0),
                        1.3,
                        accent,
                        2.7,
                    );
                }
            }
        }

        let hud_minimal = self.minimal_hud || Self::should_auto_minimize_hud(ctx.renderer);
        if !self.briefing && !self.paused && !self.victory && !self.defeat && !hud_minimal {
            let hud_scale = Self::hud_scale(ctx.renderer);
            let minimap = self.minimap_transform(ctx.renderer);
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    minimap.panel.center(),
                    minimap.panel.size() + Vec2::splat(12.0 * hud_scale),
                )
                .with_color(Color::rgba(0.04, 0.48, 0.56, 0.92))
                .with_z(7.4),
            );
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(minimap.panel.center(), minimap.panel.size())
                    .with_color(Color::rgba(0.006, 0.018, 0.035, 0.94))
                    .with_z(7.5),
            );
            // Keep authored terrain readable at a glance. This is a low-alpha
            // minimap layer, not another opaque panel, so it reinforces the
            // world-space ridge/cover treatment without hiding units or goals.
            for zone in &self.mission.terrain_zones {
                let min = minimap.world_to_panel(zone.bounds.min);
                let max = minimap.world_to_panel(zone.bounds.max);
                let center = (min + max) * 0.5;
                let size = (max - min).abs();
                let color = if zone.elevation > 0 {
                    Color::rgba(0.12, 0.82, 0.9, 0.16)
                } else {
                    Color::rgba(0.68, 0.4, 0.92, 0.14)
                };
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(center, size).with_color(color).with_z(7.65),
                );
            }
            for relay in &self.simulation.relays {
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(
                        minimap.world_to_panel(relay.position),
                        Vec2::splat(7.0 * hud_scale),
                    )
                    .with_color(if relay.active {
                        Color::rgb(0.2, 1.5, 1.2)
                    } else {
                        Color::rgb(0.45, 0.5, 0.55)
                    })
                    .with_z(8.0),
                );
            }
            for node in self.salvage_nodes.iter().filter(|node| node.remaining > 0) {
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(
                        minimap.world_to_panel(node.position),
                        Vec2::splat(6.0 * hud_scale),
                    )
                    .with_color(match node.kind {
                        ResourceKind::Salvage => Color::rgb(0.18, 1.35, 1.45),
                        ResourceKind::Flux => Color::rgb(0.75, 0.28, 1.5),
                    })
                    .with_z(8.0),
                );
            }
            if let Some((position, _)) = self.next_objective() {
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(
                        minimap.world_to_panel(position),
                        Vec2::splat(11.0 * hud_scale),
                    )
                    .with_color(Color::rgb(1.2, 0.72, 0.18))
                    .with_z(8.25),
                );
            }
            // The raid forecast is a minimap-only alert until contacts enter
            // vision. Keeping the marker at the predicted spawn point gives
            // the player a defensible direction without adding another world
            // label or permanently tinting the playfield.
            let raid_state = self.simulation.raid_state();
            if raid_state.phase == RaidPhase::Warning {
                let raid_position = minimap.world_to_panel(raid_state.spawn_position);
                let pulse = 0.78 + (t * 8.0).sin().abs() * 0.22;
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(raid_position, Vec2::splat(15.0 * hud_scale))
                        .with_color(Color::rgba(1.45, 0.12, 0.38, 0.38 * pulse))
                        .with_z(8.26),
                );
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(raid_position, Vec2::splat(7.0 * hud_scale))
                        .with_color(Color::rgb(1.65, 0.28, 0.46))
                        .with_z(8.27),
                );
            }
            for unit in self
                .simulation
                .world
                .units()
                .iter()
                .filter(|unit| unit.alive())
            {
                if unit.faction == CHOIR && self.fog.state_at(unit.position) != FogState::Visible {
                    continue;
                }
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(
                        minimap.world_to_panel(unit.position),
                        Vec2::splat(5.0 * hud_scale),
                    )
                    .with_color(if unit.faction == PLAYER {
                        Color::rgb(0.18, 1.6, 1.25)
                    } else {
                        Color::rgb(1.65, 0.12, 0.48)
                    })
                    .with_z(8.1),
                );
            }
            let visible = ctx.renderer.camera.visible_world_size();
            let camera_rect = Aabb::from_center_size(ctx.renderer.camera.position, visible);
            let map_min = minimap.world_to_panel(camera_rect.min);
            let map_max = minimap.world_to_panel(camera_rect.max);
            let center = (map_min + map_max) * 0.5;
            let size = (map_max - map_min).abs();
            for (position, dimensions) in [
                (
                    Vec2::new(center.x, map_min.y),
                    Vec2::new(size.x, 2.0 * hud_scale),
                ),
                (
                    Vec2::new(center.x, map_max.y),
                    Vec2::new(size.x, 2.0 * hud_scale),
                ),
                (
                    Vec2::new(map_min.x, center.y),
                    Vec2::new(2.0 * hud_scale, size.y),
                ),
                (
                    Vec2::new(map_max.x, center.y),
                    Vec2::new(2.0 * hud_scale, size.y),
                ),
            ] {
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(position, dimensions)
                        .with_color(Color::rgba(0.9, 1.1, 1.0, 0.9))
                        .with_z(8.2),
                );
            }

            let mouse_world = ctx
                .renderer
                .camera
                .screen_to_world(ctx.input.mouse_position);
            for slot in 1..=5 {
                let rect = Self::control_group_chip_rect(minimap.panel, slot, hud_scale);
                let count = self.simulation.world.control_group(slot).len();
                let hovered = rect.contains_point(mouse_world);
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(rect.center(), rect.size())
                        .with_color(if count > 0 {
                            if hovered {
                                Color::rgba(0.2, 0.6, 0.65, 0.95)
                            } else {
                                Color::rgba(0.05, 0.35, 0.4, 0.9)
                            }
                        } else {
                            Color::rgba(0.05, 0.08, 0.12, 0.7)
                        })
                        .with_z(8.3),
                );
                self.draw_text(
                    ctx.renderer,
                    &format!("{slot}:{count}"),
                    rect.min + Vec2::new(4.0, 8.0) * hud_scale,
                    1.6 * hud_scale,
                    Color::rgb(0.85, 0.95, 0.95),
                    8.4,
                );
            }
            if controls_hint_visible {
                let legend_origin = minimap.panel.max + Vec2::new(0.0, 72.0 * hud_scale);
                self.draw_text(
                    ctx.renderer,
                    "CYAN RIDGE  HIGH GROUND",
                    legend_origin,
                    1.15 * hud_scale,
                    Color::rgba(0.25, 1.05, 1.0, 0.9),
                    8.35,
                );
                self.draw_text(
                    ctx.renderer,
                    "VIOLET POCKET  COVER",
                    legend_origin + Vec2::new(0.0, -16.0 * hud_scale),
                    1.15 * hud_scale,
                    Color::rgba(0.75, 0.5, 1.0, 0.9),
                    8.35,
                );
            }
        }

        {
            let hud_scale = Self::hud_scale(ctx.renderer);
            let rect = Self::pause_icon_rect(ctx.renderer);
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(rect.center(), rect.size())
                    .with_color(Color::rgba(0.04, 0.08, 0.12, 0.75))
                    .with_z(9.0),
            );
            let bar_size = Vec2::new(6.0, 22.0) * hud_scale;
            for offset in [Vec2::new(-7.0, 0.0), Vec2::new(7.0, 0.0)] {
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(rect.center() + offset * hud_scale, bar_size)
                        .with_color(Color::rgb(0.85, 0.9, 0.95))
                        .with_z(9.1),
                );
            }
        }

        let hud_scale = Self::hud_scale(ctx.renderer);
        let dense_hud = Self::hud_dense_layout(ctx.renderer);
        let top_left = ctx
            .renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(0.0, 1.0))
            + Vec2::new(30.0, -34.0) * hud_scale;
        if !self.briefing && !self.victory && !self.defeat && !hud_minimal {
            // Keep only the high-value StarCraft-style strip persistent. Detailed
            // controls and action feedback are disclosed below as transient text.
            let telemetry_panel_height = if controls_hint_visible {
                132.0
            } else if dense_hud {
                84.0
            } else {
                104.0
            };
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    top_left + Vec2::new(260.0, -(telemetry_panel_height * 0.5 - 2.0)) * hud_scale,
                    Vec2::new(
                        if dense_hud { 540.0 } else { 590.0 },
                        telemetry_panel_height,
                    ) * hud_scale,
                )
                .with_color(Color::rgba(0.01, 0.025, 0.05, 0.68))
                .with_z(7.5),
            );
            let active_relays = self
                .simulation
                .relays
                .iter()
                .filter(|relay| relay.active)
                .count();
            let objective_line = match self.mission.victory {
                VictoryCondition::RestoreRelaysAndDefeatBoss { .. } => self
                    .mission_objective_progress_line()
                    .map(|specialist| {
                        format!(
                            "{}  {specialist}  RELAYS {active_relays}/{}",
                            self.mission.title,
                            self.simulation.relays.len()
                        )
                    })
                    .unwrap_or_else(|| {
                        format!(
                            "{}  RELAYS {active_relays}/{}",
                            self.mission.title,
                            self.simulation.relays.len()
                        )
                    }),
                VictoryCondition::EscortToExtraction { point, .. } => {
                    if let Some(scan_line) = self.mission_objective_progress_line() {
                        format!("{}  {scan_line}", self.mission.title)
                    } else {
                        let escort_status = self
                            .simulation
                            .escort_unit
                            .and_then(|id| self.simulation.world.unit(id))
                            .map(|unit| {
                                if unit.alive() {
                                    format!("{:.0}M TO EXTRACTION", unit.position.distance(point))
                                } else {
                                    "ESCORT LOST".to_owned()
                                }
                            })
                            .unwrap_or_else(|| "ESCORT LOST".to_owned());
                        format!("{}  {escort_status}", self.mission.title)
                    }
                }
            };
            self.draw_text(
                ctx.renderer,
                &objective_line,
                top_left,
                if dense_hud {
                    2.9 * hud_scale
                } else {
                    3.35 * hud_scale
                },
                Color::rgb(0.73, 1.15, 1.08),
                8.0,
            );
            if let Some(raid_copy) = Self::raid_hud_copy(self.simulation.raid_state()) {
                // Keep the warning in the open top-center lane. Anchoring it from
                // the title's left edge made long mission names collide with the
                // alert at the compact 1280px native viewport.
                let top_center = ctx
                    .renderer
                    .camera
                    .world_from_viewport_fraction(Vec2::new(0.5, 1.0));
                let chip_origin = top_center + Vec2::new(-95.0, -44.0) * hud_scale;
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(
                        chip_origin + Vec2::new(95.0, 0.0) * hud_scale,
                        Vec2::new(190.0, 25.0) * hud_scale,
                    )
                    .with_color(Color::rgba(0.34, 0.045, 0.11, 0.92))
                    .with_z(8.05),
                );
                self.draw_text(
                    ctx.renderer,
                    &raid_copy,
                    chip_origin + Vec2::new(8.0, -5.0) * hud_scale,
                    1.28 * hud_scale,
                    Color::rgb(1.25, 0.52, 0.55),
                    8.1,
                );
            }
            if controls_hint_visible && !dense_hud {
                let selection_count = self.simulation.world.selection().ids().len();
                let control_hint = if selection_count == 0 {
                    "DRAG SELECT  •  TERRAIN MOVE  •  F1 HELP"
                } else if selection_count == 1
                    && self.selected_single_unit_kind() == Some(UnitKind::Surveyor)
                {
                    "G HARVEST  •  RIGHT CLICK MOVE  •  Y SCAN  •  F1 HELP"
                } else {
                    "RIGHT CLICK MOVE  •  SHIFT QUEUE  •  Y SPECIAL  •  F1 HELP"
                };
                self.draw_text(
                    ctx.renderer,
                    control_hint,
                    top_left + Vec2::new(0.0, -25.0) * hud_scale,
                    1.9 * hud_scale,
                    Color::rgba(0.58, 0.7, 0.78, 0.86),
                    8.0,
                );
            }
            if controls_hint_visible && dense_hud {
                self.draw_text(
                    ctx.renderer,
                    "F1 FOR CONTROL HINT",
                    top_left + Vec2::new(0.0, -25.0) * hud_scale,
                    1.45 * hud_scale,
                    Color::rgba(0.55, 0.7, 0.8, 0.9),
                    8.0,
                );
            }
            let income = active_relays * self.relay_income() as usize;
            let cargo: u32 = self.harvest_jobs.values().map(|job| job.cargo).sum();
            let resource_line = match (cargo > 0, self.lumen_cores > 0) {
                (false, false) => format!(
                    "SALVAGE {}  FLUX {}",
                    self.simulation.resources.amount(),
                    self.simulation.flux
                ),
                (true, false) => format!(
                    "SALVAGE {}  FLUX {}  CARGO {cargo}",
                    self.simulation.resources.amount(),
                    self.simulation.flux
                ),
                (false, true) => format!(
                    "SALVAGE {}  FLUX {}  CORES {}",
                    self.simulation.resources.amount(),
                    self.simulation.flux,
                    self.lumen_cores
                ),
                (true, true) => format!(
                    "SALVAGE {}  FLUX {}  CARGO {cargo}  CORES {}",
                    self.simulation.resources.amount(),
                    self.simulation.flux,
                    self.lumen_cores
                ),
            };
            if dense_hud {
                self.draw_text(
                    ctx.renderer,
                    &format!(
                        "{}  IN +{income}/S  RELAYS {}/{}",
                        resource_line,
                        active_relays + 1,
                        self.simulation.relays.len() + 1
                    ),
                    top_left + Vec2::new(0.0, -52.0) * hud_scale,
                    2.15 * hud_scale,
                    Color::rgb(0.96, 0.72, 0.28),
                    8.0,
                );
            } else {
                self.draw_text(
                    ctx.renderer,
                    &resource_line,
                    top_left + Vec2::new(0.0, -50.0) * hud_scale,
                    2.8 * hud_scale,
                    Color::rgb(0.96, 0.72, 0.28),
                    8.0,
                );
                self.draw_text(
                    ctx.renderer,
                    &format!(
                        "IN +{income}/S  POWER {}/{}  SUPPLY {}/{}",
                        active_relays + 1,
                        self.simulation.relays.len() + 1,
                        self.simulation.supply.used(),
                        self.simulation.supply.capacity()
                    ),
                    top_left + Vec2::new(0.0, -75.0) * hud_scale,
                    2.35 * hud_scale,
                    Color::rgb(0.96, 0.72, 0.28),
                    8.0,
                );
            }
            if !dense_hud {
                if let Some(idle_copy) = self.idle_surveyor_hud_copy() {
                    // This chip lives in the unused right side of the telemetry
                    // panel. Its bounded copy keeps the global resource line
                    // readable and makes the alert disappear as soon as a route
                    // is assigned.
                    let chip_origin = top_left + Vec2::new(366.0, -49.0) * hud_scale;
                    ctx.renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(
                            chip_origin + Vec2::new(105.0, 0.0) * hud_scale,
                            Vec2::new(218.0, 24.0) * hud_scale,
                        )
                        .with_color(Color::rgba(0.34, 0.15, 0.055, 0.92))
                        .with_z(8.05),
                    );
                    self.draw_text(
                        ctx.renderer,
                        &idle_copy,
                        chip_origin + Vec2::new(8.0, -5.0) * hud_scale,
                        1.25 * hud_scale,
                        Color::rgb(1.2, 0.72, 0.28),
                        8.1,
                    );
                }
                if let Some((_target, objective_copy, accent)) = self.resource_objective_hud_copy()
                {
                    // Resource objectives use a second, lower exception lane so
                    // progress never collides with the global salvage/income
                    // readout. The fixed 252px shell matches the formatter's
                    // 32-character cap at the compact HUD text scale.
                    let chip_origin = top_left + Vec2::new(338.0, -100.0) * hud_scale;
                    ctx.renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(
                            chip_origin + Vec2::new(126.0, 0.0) * hud_scale,
                            Vec2::new(252.0, 24.0) * hud_scale,
                        )
                        .with_color(Color::rgba(0.025, 0.07, 0.09, 0.94))
                        .with_z(8.05),
                    );
                    ctx.renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(
                            chip_origin + Vec2::new(3.0, 0.0) * hud_scale,
                            Vec2::new(5.0, 18.0) * hud_scale,
                        )
                        .with_color(accent)
                        .with_z(8.1),
                    );
                    self.draw_text(
                        ctx.renderer,
                        &objective_copy,
                        chip_origin + Vec2::new(14.0, -5.0) * hud_scale,
                        1.2 * hud_scale,
                        accent,
                        8.15,
                    );
                }
            }
        }
        if let Some(selected) = self.simulation.world.selection().ids().first() {
            let count = self.simulation.world.selection().ids().len();
            let kind = self.simulation.kinds[selected];
            if self
                .simulation
                .world
                .unit(*selected)
                .is_some_and(|unit| unit.faction == PLAYER)
            {
                let unit_card = Self::unit_card_origin(ctx.renderer);
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(
                        unit_card + Vec2::new(210.0, 58.0) * hud_scale,
                        Vec2::new(420.0, 116.0) * hud_scale,
                    )
                    .with_color(Color::rgba(0.01, 0.025, 0.05, 0.88))
                    .with_z(7.7),
                );
                let portrait_center = unit_card + Vec2::new(58.0, 58.0) * hud_scale;
                let portrait_size = Vec2::splat(108.0 * hud_scale);
                match Self::unit_card_portrait(kind, false) {
                    UnitCardPortrait::Command(frame) => {
                        let portrait =
                            self.portrait_atlas
                                .sprite(portrait_center, portrait_size, frame);
                        ctx.renderer
                            .draw_sprite(self.tex_portraits, portrait.with_z(8.1));
                    }
                    UnitCardPortrait::Tactical(frame) => {
                        let portrait =
                            self.unit_atlas
                                .sprite(portrait_center, portrait_size, frame);
                        ctx.renderer
                            .draw_sprite(self.tex_units, portrait.with_z(8.1));
                    }
                }
                if let Some(unit) = self.simulation.world.unit(*selected) {
                    let compact_unit_card = dense_hud;
                    let identity = self.unit_identity_label(*selected, kind);
                    self.draw_text(
                        ctx.renderer,
                        &format!(
                            "HP {:03}/{:03}  //  {}",
                            unit.health.ceil() as u32,
                            unit.max_health.ceil() as u32,
                            Self::order_label(unit.order)
                        ),
                        unit_card + Vec2::new(122.0, 73.0) * hud_scale,
                        1.9 * hud_scale,
                        Color::rgb(0.72, 0.92, 0.9),
                        8.1,
                    );
                    let role_text = match kind {
                        UnitKind::Warden if count == 1 && compact_unit_card => {
                            format!("{identity} // {}", kind.role().label())
                        }
                        UnitKind::Engineer if count == 1 && compact_unit_card => {
                            format!("{identity} // {}", kind.role().label())
                        }
                        UnitKind::Surveyor if count == 1 && compact_unit_card => {
                            format!("{identity} // {}", kind.role().label())
                        }
                        UnitKind::Warden if count == 1 => {
                            format!("{identity} // {} // SURGE", kind.role().label())
                        }
                        UnitKind::Engineer if count == 1 => {
                            format!("{identity} // {} // REPAIR", kind.role().label())
                        }
                        UnitKind::Surveyor if count == 1 => {
                            format!("{identity} // {} // SCAN", kind.role().label())
                        }
                        _ if count > 1 => "MIXED LANTERN SQUAD".to_owned(),
                        _ => "CONTACT // HOSTILE".to_owned(),
                    };
                    self.draw_text(
                        ctx.renderer,
                        &role_text,
                        unit_card + Vec2::new(122.0, 48.0) * hud_scale,
                        if compact_unit_card {
                            1.35 * hud_scale
                        } else {
                            1.55 * hud_scale
                        },
                        Color::rgba(0.55, 0.75, 0.78, 0.9),
                        8.1,
                    );
                    if count > 1 && !compact_unit_card {
                        let mut role_counts = [0_u32; 3];
                        for id in self.simulation.world.selection().ids() {
                            match self.simulation.kinds.get(id).copied() {
                                Some(UnitKind::Warden) => role_counts[0] += 1,
                                Some(UnitKind::Engineer) => role_counts[1] += 1,
                                Some(UnitKind::Surveyor) => role_counts[2] += 1,
                                _ => {}
                            }
                        }
                        self.draw_text(
                            ctx.renderer,
                            &self.mixed_squad_role_line(role_counts),
                            unit_card + Vec2::new(122.0, 14.0) * hud_scale,
                            1.25 * hud_scale,
                            Color::rgba(0.66, 0.82, 0.84, 0.9),
                            8.1,
                        );
                        for (index, id) in self
                            .simulation
                            .world
                            .selection()
                            .ids()
                            .iter()
                            .take(5)
                            .enumerate()
                        {
                            let Some(kind) = self.simulation.kinds.get(id).copied() else {
                                continue;
                            };
                            let chip = Self::selection_chip_rect(ctx.renderer, index);
                            ctx.renderer.draw_sprite(
                                self.tex_ui,
                                Sprite::new(chip.center(), chip.size())
                                    .with_color(Color::rgba(0.06, 0.24, 0.28, 0.88))
                                    .with_z(8.05),
                            );
                            let portrait = self.portrait_atlas.sprite(
                                chip.center(),
                                Vec2::splat(32.0 * hud_scale),
                                Self::unit_portrait_frame(kind),
                            );
                            ctx.renderer
                                .draw_sprite(self.tex_portraits, portrait.with_z(8.1));
                        }
                    }
                    if count == 1 && !compact_unit_card {
                        if let Some(ability) = MissionSimulation::ability_for_kind(kind) {
                            let cooldown = self.simulation.ability_cooldown(*selected);
                            let ability_text = if cooldown > 0.0 {
                                format!(
                                    "Y  {} // RECHARGE {:02}s",
                                    ability.label(),
                                    cooldown.ceil() as u32
                                )
                            } else {
                                "Y  ABILITY READY".to_owned()
                            };
                            self.draw_text(
                                ctx.renderer,
                                &ability_text,
                                unit_card + Vec2::new(122.0, 24.0) * hud_scale,
                                1.35 * hud_scale,
                                if cooldown > 0.0 {
                                    Color::rgba(0.58, 0.68, 0.72, 0.9)
                                } else {
                                    Color::rgb(1.05, 0.76, 0.28)
                                },
                                8.1,
                            );
                        }
                        if let Some((terrain_copy, terrain_accent)) =
                            self.terrain_readout_copy(unit.position)
                        {
                            let terrain_origin = unit_card + Vec2::new(122.0, 3.0) * hud_scale;
                            ctx.renderer.draw_sprite(
                                self.tex_ui,
                                Sprite::new(
                                    terrain_origin + Vec2::new(132.0, 0.0) * hud_scale,
                                    Vec2::new(264.0, 20.0) * hud_scale,
                                )
                                .with_color(Color::rgba(0.015, 0.045, 0.06, 0.9))
                                .with_z(8.0),
                            );
                            self.draw_text(
                                ctx.renderer,
                                &terrain_copy,
                                terrain_origin + Vec2::new(7.0, -4.0) * hud_scale,
                                1.15 * hud_scale,
                                terrain_accent,
                                8.1,
                            );
                        }
                    }
                }
            }
        }

        // A hostile unit is not selectable through the normal player input
        // path, but debug/spectator selection and future replay tooling can
        // still place one in the world selection buffer. Give that contact a
        // truthful read-only card instead of showing the Lumen portrait as a
        // generic fallback. The card intentionally exposes no player verbs.
        if let Some(selected) = self.simulation.world.selection().ids().first() {
            let count = self.simulation.world.selection().ids().len();
            let kind = self.simulation.kinds[selected];
            if count == 1
                && self
                    .simulation
                    .world
                    .unit(*selected)
                    .is_some_and(|unit| unit.faction == CHOIR && unit.alive())
            {
                let unit_card = Self::unit_card_origin(ctx.renderer);
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(
                        unit_card + Vec2::new(210.0, 58.0) * hud_scale,
                        Vec2::new(420.0, 116.0) * hud_scale,
                    )
                    .with_color(Color::rgba(0.08, 0.018, 0.075, 0.9))
                    .with_z(7.7),
                );
                let portrait_center = unit_card + Vec2::new(58.0, 58.0) * hud_scale;
                let portrait_size = Vec2::splat(108.0 * hud_scale);
                match Self::unit_card_portrait(kind, true) {
                    UnitCardPortrait::Command(frame) => {
                        let portrait =
                            self.portrait_atlas
                                .sprite(portrait_center, portrait_size, frame);
                        ctx.renderer
                            .draw_sprite(self.tex_portraits, portrait.with_z(8.1));
                    }
                    UnitCardPortrait::Tactical(frame) => {
                        let portrait =
                            self.unit_atlas
                                .sprite(portrait_center, portrait_size, frame);
                        ctx.renderer
                            .draw_sprite(self.tex_units, portrait.with_z(8.1));
                    }
                }
                if let Some(unit) = self.simulation.world.unit(*selected) {
                    self.draw_text(
                        ctx.renderer,
                        &format!(
                            "HP {:03}/{:03}  //  {}",
                            unit.health.ceil() as u32,
                            unit.max_health.ceil() as u32,
                            Self::order_label(unit.order)
                        ),
                        unit_card + Vec2::new(122.0, 73.0) * hud_scale,
                        1.9 * hud_scale,
                        Color::rgb(1.0, 0.62, 0.82),
                        8.1,
                    );
                    self.draw_text(
                        ctx.renderer,
                        &format!("CONTACT // {}", kind.role().label()),
                        unit_card + Vec2::new(122.0, 48.0) * hud_scale,
                        1.55 * hud_scale,
                        Color::rgba(1.0, 0.4, 0.74, 0.95),
                        8.1,
                    );
                    self.draw_text(
                        ctx.renderer,
                        "HOSTILE CONTACT // NO COMMAND",
                        unit_card + Vec2::new(122.0, 23.0) * hud_scale,
                        1.25 * hud_scale,
                        Color::rgba(0.82, 0.64, 0.76, 0.9),
                        8.1,
                    );
                }
            }
        }

        if let Some((speaker, line, _)) = self.radio_message {
            let top_right = ctx
                .renderer
                .camera
                .world_from_viewport_fraction(Vec2::new(1.0, 1.0));
            let slide = self.radio_pop_in.clamp(0.0, 1.0) * 42.0;
            let origin = top_right + Vec2::new(-540.0 + slide, -42.0) * hud_scale;
            let accent = Self::speaker_accent(speaker);
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    origin + Vec2::new(250.0, -36.0) * hud_scale,
                    Vec2::new(520.0, 108.0) * hud_scale,
                )
                .with_color(Color::rgba(0.025, 0.055, 0.085, 0.9))
                .with_z(8.6),
            );
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    origin + Vec2::new(7.0, -36.0) * hud_scale,
                    Vec2::new(6.0, 80.0) * hud_scale,
                )
                .with_color(accent)
                .with_z(8.7),
            );
            let portrait = self.portrait_atlas.sprite(
                origin + Vec2::new(40.0, -34.0) * hud_scale,
                Vec2::new(76.0, 76.0) * hud_scale,
                Self::speaker_portrait_frame(speaker),
            );
            ctx.renderer
                .draw_sprite(self.tex_portraits, portrait.with_z(8.75));
            let [line_one, line_two] = Self::radio_line_chunks(line);
            self.draw_text(
                ctx.renderer,
                &format!("COMMS // {speaker}"),
                origin + Vec2::new(88.0, 0.0) * hud_scale,
                1.9 * hud_scale,
                accent,
                8.8,
            );
            self.draw_text(
                ctx.renderer,
                Self::speaker_role_label(speaker),
                origin + Vec2::new(88.0, -18.0) * hud_scale,
                1.05 * hud_scale,
                Color::rgba(0.58, 0.74, 0.8, 0.9),
                8.8,
            );
            self.draw_text(
                ctx.renderer,
                &line_one,
                origin + Vec2::new(88.0, -37.0) * hud_scale,
                1.25 * hud_scale,
                Color::rgb(0.88, 0.92, 0.92),
                8.8,
            );
            if !line_two.is_empty() {
                self.draw_text(
                    ctx.renderer,
                    &line_two,
                    origin + Vec2::new(88.0, -50.0) * hud_scale,
                    1.25 * hud_scale,
                    Color::rgb(0.88, 0.92, 0.92),
                    8.8,
                );
            }
            let inbox_count = self.radio_queue.len() + self.radio_priority_queue.len();
            let inbox_priority = if self.radio_priority_queue.is_empty() {
                ""
            } else {
                " // PRIORITY"
            };
            self.draw_text(
                ctx.renderer,
                &format!("SPACE FOCUS  //  INBOX {inbox_count}{inbox_priority}"),
                origin + Vec2::new(88.0, -69.0) * hud_scale,
                1.15 * hud_scale,
                Color::rgba(0.55, 0.78, 0.82, 0.88),
                8.8,
            );
        }

        // Action feedback is a transient toast, not a second permanent
        // telemetry column. It appears only while a command is fresh, then
        // yields the playfield back to the player.
        let transient_message = self
            .status
            .as_ref()
            .map(|(message, _)| message.clone())
            .or_else(|| {
                controls_hint_visible
                    .then(|| self.engineer_relay_status())
                    .flatten()
            });
        if let Some(message) = transient_message.as_deref() {
            let toast_origin = top_left + Vec2::new(300.0, -112.0) * hud_scale;
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    toast_origin + Vec2::new(238.0, 0.0) * hud_scale,
                    Vec2::new(476.0, 28.0) * hud_scale,
                )
                .with_color(Color::rgba(0.01, 0.035, 0.055, 0.86))
                .with_z(8.45),
            );
            self.draw_text(
                ctx.renderer,
                message,
                toast_origin + Vec2::new(12.0, -5.0) * hud_scale,
                1.65 * hud_scale,
                Color::rgb(0.95, 0.82, 0.42),
                8.5,
            );
        }

        if !self.briefing
            && !self.paused
            && !self.victory
            && !self.defeat
            && self.command_card_visible()
        {
            let compact_card = self.command_card_compact || self.minimal_hud || dense_hud;
            let card_text = Self::command_card_text_origin(ctx.renderer);
            let visible_rows = self.visible_command_card_rows_for_display(ctx.renderer);
            let panel_size = if compact_card {
                Vec2::new(310.0, (visible_rows.len().max(1) as f32 * 30.0) + 104.0)
            } else {
                Vec2::new(310.0, (visible_rows.len().max(1) as f32 * 30.0) + 104.0)
            };
            let card_center = card_text
                + Vec2::new(155.0, if compact_card { -132.0 } else { -132.5 }) * hud_scale;
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(card_center, panel_size * hud_scale)
                    .with_color(Color::rgba(0.01, 0.025, 0.05, 0.88))
                    .with_z(7.5),
            );
            let card_title = if self.selected_resource_node.is_some() {
                "RESOURCE NODE".to_owned()
            } else {
                match self.selected_structure {
                    Some(StructureKind::Relay(_)) => "POWER RELAY".to_owned(),
                    Some(StructureKind::Reactor) => "AUXILIARY REACTOR".to_owned(),
                    Some(StructureKind::Fabricator) => self.fabricator_card_title(),
                    None => match self.selected_single_unit_kind() {
                        Some(kind) => self
                            .selected_unit_id()
                            .map(|id| {
                                format!(
                                    "{} // {} COMMAND",
                                    self.unit_identity_label(id, kind),
                                    kind.label()
                                )
                            })
                            .unwrap_or_else(|| "LANTERN SQUAD COMMAND".to_owned()),
                        None if !self.simulation.world.selection().ids().is_empty() => {
                            "LANTERN SQUAD COMMAND".to_owned()
                        }
                        _ => "LANTERN FABRICATOR".to_owned(),
                    },
                }
            };
            self.draw_text(
                ctx.renderer,
                &card_title,
                card_text,
                if compact_card {
                    2.3 * hud_scale
                } else {
                    2.8 * hud_scale
                },
                Color::rgb(0.3, 1.4, 1.2),
                8.0,
            );
            let mouse_world = ctx
                .renderer
                .camera
                .screen_to_world(ctx.input.mouse_position);
            for (slot, &index) in visible_rows.iter().enumerate() {
                let rect = Self::command_card_row_rect(card_text, slot, hud_scale);
                let hovered = rect.contains_point(mouse_world);
                let available = self.command_card_available(index);
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(rect.center(), rect.size())
                        .with_color(if !available {
                            Color::rgba(0.03, 0.06, 0.08, 0.22)
                        } else if hovered {
                            Color::rgba(0.16, 0.55, 0.6, 0.35)
                        } else {
                            Color::rgba(0.04, 0.08, 0.12, 0.3)
                        })
                        .with_z(7.6),
                );
                let command_label = self.command_card_display(index);
                self.draw_text(
                    ctx.renderer,
                    &command_label,
                    rect.min + Vec2::new(8.0, 8.0) * hud_scale,
                    if compact_card {
                        1.42 * hud_scale
                    } else {
                        1.65 * hud_scale
                    },
                    if available {
                        Color::rgb(0.88, 0.92, 0.92)
                    } else {
                        Color::rgba(0.48, 0.58, 0.62, 0.82)
                    },
                    8.0,
                );
            }
            if controls_hint_visible {
                if !dense_hud {
                    self.draw_text(
                        ctx.renderer,
                        "CMD/CTRL+1-5 ASSIGN   1-5 OR CLICK RECALL",
                        card_text + Vec2::new(0.0, -142.0) * hud_scale,
                        1.4 * hud_scale,
                        Color::rgba(0.55, 0.7, 0.78, 0.9),
                        8.0,
                    );
                } else {
                    self.draw_text(
                        ctx.renderer,
                        "CTRL+1-5 ASSIGN   1-5 RECALL",
                        card_text + Vec2::new(0.0, -142.0) * hud_scale,
                        1.35 * hud_scale,
                        Color::rgba(0.58, 0.73, 0.84, 0.86),
                        8.0,
                    );
                }
            } else if self.command_card_should_paginate(ctx.renderer) {
                let page = self.command_card_visible_page() + 1;
                let pages = self.command_card_page_count();
                self.draw_text(
                    ctx.renderer,
                    &format!("PAGE {page} / {pages}  // ARROWS PAGINATE"),
                    card_text + Vec2::new(0.0, -112.0) * hud_scale,
                    1.35 * hud_scale,
                    Color::rgba(0.55, 0.78, 0.9, 0.9),
                    8.0,
                );
            }
            let front_progress = (matches!(
                self.selected_structure,
                None | Some(StructureKind::Fabricator)
            ) && self.selected_resource_node.is_none())
            .then(|| {
                self.simulation
                    .production
                    .items()
                    .front()
                    .map(|item| item.progress())
            })
            .flatten();
            let queue_label = if let Some(node) = self.selected_resource_node {
                Some(self.resource_node_status_line(node))
            } else {
                match self.selected_structure {
                    Some(
                        structure @ (StructureKind::Relay(_)
                        | StructureKind::Reactor
                        | StructureKind::Fabricator),
                    ) => Some(self.structure_status_line(structure)),
                    _ => self
                        .simulation
                        .production
                        .items()
                        .front()
                        .map(|item| {
                            let label = UnitKind::from_product(item.product)
                                .map(UnitKind::label)
                                .unwrap_or("UNKNOWN");
                            Some(format!(
                                "BUILDING {label}  {:02}%  QUEUE {}",
                                (item.progress() * 100.0) as u32,
                                self.simulation.production.items().len()
                            ))
                        })
                        .unwrap_or(None),
                }
            };
            if let Some(queue_label) = queue_label {
                let queue_label_pixel =
                    if matches!(self.selected_structure, Some(StructureKind::Fabricator)) {
                        1.65
                    } else {
                        2.0
                    };
                if !dense_hud {
                    self.draw_text(
                        ctx.renderer,
                        &queue_label,
                        card_text + Vec2::new(0.0, -166.0) * hud_scale,
                        queue_label_pixel * hud_scale,
                        Color::rgb(1.15, 0.7, 0.25),
                        8.0,
                    );
                }
            }
            if let Some(progress) = front_progress {
                if !dense_hud {
                    let bar_origin = card_text + Vec2::new(0.0, -188.0) * hud_scale;
                    ctx.renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(
                            bar_origin + Vec2::new(150.0, 0.0) * hud_scale,
                            Vec2::new(300.0, 8.0) * hud_scale,
                        )
                        .with_color(Color::rgba(0.1, 0.1, 0.12, 0.9))
                        .with_z(8.0),
                    );
                    ctx.renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(
                            bar_origin + Vec2::new(300.0 * progress * 0.5, 0.0) * hud_scale,
                            Vec2::new(300.0 * progress, 8.0) * hud_scale,
                        )
                        .with_color(Color::rgb(1.15, 0.7, 0.25))
                        .with_z(8.1),
                    );
                }
            }
        }

        let view = ctx.renderer.camera.visible_world_size();
        let overlay: Option<(&str, &str, String, Color)> = if self.briefing {
            Some((
                self.mission.title,
                self.mission.briefing_story,
                "SPACE / CLICK A ROW TO DEPLOY".to_owned(),
                Color::rgb(0.32, 1.55, 1.35),
            ))
        } else if self.paused {
            Some((
                "TACTICAL PAUSE",
                "ORDERS SUSPENDED",
                "ESC TO RESUME".to_owned(),
                Color::rgb(0.85, 0.85, 0.9),
            ))
        } else if self.victory {
            let prompt = self
                .status
                .as_ref()
                .map(|(message, _)| message.clone())
                .unwrap_or_else(|| "MISSION COMPLETE".to_owned());
            Some((
                self.mission.victory_title,
                self.mission.victory_story,
                prompt,
                Color::rgb(0.3, 1.5, 1.0),
            ))
        } else if self.defeat {
            Some((
                self.mission.defeat_title,
                self.mission.defeat_story,
                "SPACE / ENTER TO RETRY — ESC TO MISSIONS".to_owned(),
                Color::rgb(1.4, 0.4, 0.35),
            ))
        } else {
            None
        };
        if let Some((title, story, prompt, title_color)) = overlay {
            let overlay_scale = Self::hud_scale(ctx.renderer);
            self.draw_full_screen_backdrop(ctx, Color::rgba(0.01, 0.02, 0.045, 0.8));
            let center = ctx.renderer.camera.position;
            self.draw_text_shadowed(
                ctx.renderer,
                title,
                center + Vec2::new(-view.x * 0.42, view.y * 0.36),
                6.5 * overlay_scale,
                title_color,
                11.0,
            );
            if self.victory {
                let survivors = self
                    .simulation
                    .world
                    .units()
                    .iter()
                    .filter(|unit| unit.faction == PLAYER && unit.alive())
                    .count();
                let debrief_origin = center
                    + Vec2::new(-view.x * 0.42, view.y * 0.36)
                    + Vec2::new(0.0, -112.0) * overlay_scale;
                self.draw_text_shadowed(
                    ctx.renderer,
                    &format!(
                        "DEBRIEF // +{} LUMEN  •  {} LANTERNS SURVIVED  •  MISSION {} UNLOCKED",
                        self.mission.reward_lumen, survivors, self.mission.unlock_next
                    ),
                    debrief_origin,
                    2.2 * overlay_scale,
                    Color::rgb(0.96, 0.72, 0.28),
                    11.0,
                );
                self.draw_text_shadowed(
                    ctx.renderer,
                    self.campaign_consequence(),
                    debrief_origin + Vec2::new(0.0, -34.0) * overlay_scale,
                    2.0 * overlay_scale,
                    Color::rgb(0.32, 1.35, 1.18),
                    11.0,
                );
            }
            if self.briefing {
                let header_origin =
                    center + Vec2::new(-view.x * 0.42, view.y * 0.36) * overlay_scale;
                let speaker = self.briefing_speaker();
                let accent = Self::speaker_accent(speaker);
                let card_center = header_origin + Vec2::new(66.0, -108.0) * overlay_scale;
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(card_center, Vec2::new(172.0, 154.0) * overlay_scale)
                        .with_color(Color::rgba(0.025, 0.07, 0.1, 0.92))
                        .with_z(10.05),
                );
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(
                        card_center + Vec2::new(-81.0, 0.0) * overlay_scale,
                        Vec2::new(6.0, 132.0) * overlay_scale,
                    )
                    .with_color(accent)
                    .with_z(10.1),
                );
                let portrait = self.portrait_atlas.sprite(
                    card_center + Vec2::new(0.0, 12.0) * overlay_scale,
                    Vec2::splat(96.0 * overlay_scale),
                    Self::speaker_portrait_frame(speaker),
                );
                ctx.renderer
                    .draw_sprite(self.tex_portraits, portrait.with_z(10.2));
                self.draw_text_shadowed(
                    ctx.renderer,
                    speaker,
                    card_center + Vec2::new(-70.0, -56.0) * overlay_scale,
                    1.7 * overlay_scale,
                    accent,
                    10.3,
                );
                self.draw_text_shadowed(
                    ctx.renderer,
                    Self::speaker_role_label(speaker),
                    card_center + Vec2::new(-70.0, -73.0) * overlay_scale,
                    1.2 * overlay_scale,
                    Color::rgba(0.6, 0.76, 0.8, 0.92),
                    10.3,
                );

                let [line_one, line_two, line_three] =
                    Self::briefing_story_chunks(self.briefing_story_copy());
                let story_origin = header_origin + Vec2::new(150.0, -75.0) * overlay_scale;
                self.draw_text_shadowed(
                    ctx.renderer,
                    "MISSION BRIEF // INCOMING",
                    story_origin,
                    1.55 * overlay_scale,
                    accent,
                    10.3,
                );
                for (index, line) in [line_one, line_two, line_three].iter().enumerate() {
                    if line.is_empty() {
                        continue;
                    }
                    self.draw_text_shadowed(
                        ctx.renderer,
                        line,
                        story_origin + Vec2::new(0.0, -25.0 - index as f32 * 17.0) * overlay_scale,
                        2.15 * overlay_scale,
                        Color::rgb(0.82, 0.9, 0.92),
                        10.3,
                    );
                }
            } else {
                self.draw_text_shadowed(
                    ctx.renderer,
                    story,
                    center
                        + Vec2::new(-view.x * 0.42, view.y * 0.36)
                        + Vec2::new(0.0, -55.0) * overlay_scale,
                    2.1 * overlay_scale,
                    Color::rgb(0.8, 0.88, 0.9),
                    11.0,
                );
            }
            self.draw_text_shadowed(
                ctx.renderer,
                &prompt,
                center + Vec2::new(-view.x * 0.42, -view.y * 0.4),
                3.2 * overlay_scale,
                Color::rgb(1.25, 0.78, 0.28),
                11.0,
            );
            if self.briefing {
                self.draw_text_shadowed(
                    ctx.renderer,
                    &format!("LUMEN  {}", self.save_data.campaign.currency),
                    center + Vec2::new(view.x * 0.22, view.y * 0.42) * overlay_scale,
                    1.8 * overlay_scale,
                    Color::rgba(0.75, 0.9, 0.95, 0.95),
                    11.0,
                );
                self.draw_text_shadowed(
                    ctx.renderer,
                    "FIELD SYSTEMS",
                    center + Vec2::new(-310.0, 78.0) * overlay_scale,
                    1.25 * overlay_scale,
                    Color::rgba(0.55, 0.8, 0.84, 0.92),
                    11.0,
                );
                self.draw_text_shadowed(
                    ctx.renderer,
                    "ROSTER / ACCORDS",
                    center + Vec2::new(110.0, 78.0) * overlay_scale,
                    1.25 * overlay_scale,
                    Color::rgba(0.55, 0.8, 0.84, 0.92),
                    11.0,
                );
                let mouse_world = ctx
                    .renderer
                    .camera
                    .screen_to_world(ctx.input.mouse_position);
                for (index, (_, label, color)) in self.briefing_rows().iter().enumerate() {
                    let rect = Self::briefing_row_rect(center, index, overlay_scale);
                    let hovered = rect.contains_point(mouse_world);
                    ctx.renderer.draw_sprite(
                        self.tex_ui,
                        Sprite::new(rect.center(), rect.size())
                            .with_color(if hovered {
                                Color::rgba(0.16, 0.55, 0.6, 0.4)
                            } else {
                                Color::rgba(0.04, 0.08, 0.12, 0.4)
                            })
                            .with_z(10.0),
                    );
                    self.draw_text_shadowed(
                        ctx.renderer,
                        label,
                        rect.min + Vec2::new(14.0, 10.0) * overlay_scale,
                        Self::briefing_label_scale(label, overlay_scale),
                        *color,
                        11.0,
                    );
                }
            }
        }
    }
}

fn main() {
    run(LastLight::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directional_art_keeps_authored_front_aligned_with_screen_down() {
        assert!((unit_sprite_rotation(Vec2::new(0.0, -100.0))).abs() < 1e-5);
        assert!(
            (unit_sprite_rotation(Vec2::new(100.0, 0.0)) - std::f32::consts::FRAC_PI_2).abs()
                < 1e-5
        );
    }

    #[test]
    fn directional_art_turns_away_from_the_old_upside_down_heading() {
        assert!((unit_sprite_rotation(Vec2::new(0.0, 100.0)) - std::f32::consts::PI).abs() < 1e-5);
        assert_eq!(unit_sprite_rotation(Vec2::new(0.2, 0.2)), 0.0);
    }

    #[test]
    fn hostile_unit_cards_use_distinct_tactical_atlas_frames() {
        assert_eq!(
            LastLight::unit_card_portrait(UnitKind::Needle, true),
            UnitCardPortrait::Tactical(UnitKind::Needle.atlas_frame())
        );
        assert_eq!(
            LastLight::unit_card_portrait(UnitKind::Canticle, true),
            UnitCardPortrait::Tactical(UnitKind::Canticle.atlas_frame())
        );
        assert_eq!(
            LastLight::unit_card_portrait(UnitKind::BellMine, true),
            UnitCardPortrait::Tactical(UnitKind::BellMine.atlas_frame())
        );
        assert_eq!(
            LastLight::unit_card_portrait(UnitKind::Engineer, false),
            UnitCardPortrait::Command(LastLight::unit_portrait_frame(UnitKind::Engineer))
        );
    }

    #[test]
    fn surveyor_scan_pose_requires_active_scan_or_extraction() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let surveyor = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .expect("reclaim starts with a Surveyor");
        let position = game.simulation.world.unit(surveyor).unwrap().position;

        assert_eq!(
            game.unit_animation_state(surveyor, UnitKind::Surveyor, true, Vec2::ZERO, false),
            UnitAnimationState::Idle
        );
        game.simulation.scan_pulse = Some((position, 2.0));
        assert_eq!(
            game.unit_animation_state(surveyor, UnitKind::Surveyor, true, Vec2::ZERO, false),
            UnitAnimationState::Scan
        );
        game.simulation.scan_pulse = Some((position + Vec2::X * 2.0, 2.0));
        assert_eq!(
            game.unit_animation_state(surveyor, UnitKind::Surveyor, true, Vec2::ZERO, false),
            UnitAnimationState::Idle
        );
        game.simulation.scan_pulse = None;
        game.harvest_jobs.insert(
            surveyor,
            HarvestJob {
                node: 0,
                cargo: 0,
                phase: HarvestPhase::Extracting,
            },
        );
        assert_eq!(
            game.unit_animation_state(surveyor, UnitKind::Surveyor, true, Vec2::ZERO, false),
            UnitAnimationState::Scan
        );
        game.harvest_jobs.get_mut(&surveyor).unwrap().phase = HarvestPhase::ToDepot;
        assert_eq!(
            game.unit_animation_state(surveyor, UnitKind::Surveyor, true, Vec2::ZERO, false),
            UnitAnimationState::Idle
        );
    }

    #[test]
    fn engineer_repair_pose_precedes_movement_and_warden_attack_uses_authored_strip() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let engineer = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Engineer).then_some(*id))
            .expect("reclaim starts with an Engineer");
        let warden = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("reclaim starts with a Warden");
        let engineer_position = game.simulation.world.unit(engineer).unwrap().position;
        game.simulation.world.unit_mut(warden).unwrap().position = engineer_position;
        game.simulation.world.unit_mut(warden).unwrap().health = 10.0;
        game.simulation.world.unit_mut(engineer).unwrap().velocity = Vec2::X * 20.0;

        assert_eq!(
            game.unit_animation_state(engineer, UnitKind::Engineer, true, Vec2::X * 20.0, false),
            UnitAnimationState::Repair
        );
        assert_eq!(
            game.unit_animation_clip(UnitKind::Warden, UnitAnimationState::Attack)
                .map(|clip| (clip.frames.len(), clip.fps, clip.looping)),
            Some((5, 12.0, true))
        );
        assert!(matches!(
            game.unit_animation_atlas(UnitKind::Warden, UnitAnimationState::Attack),
            Some((_, atlas)) if atlas.columns == 5 && atlas.rows == 1
        ));
    }

    #[test]
    fn build_and_mark_states_are_role_specific_procedural_cues() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let engineer = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Engineer).then_some(*id))
            .expect("reclaim starts with an Engineer");
        let surveyor = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .expect("reclaim starts with a Surveyor");

        game.placing_beacon = true;
        assert_eq!(
            game.unit_animation_state(engineer, UnitKind::Engineer, true, Vec2::ZERO, false),
            UnitAnimationState::Build
        );
        assert_eq!(
            game.unit_animation_clip(UnitKind::Engineer, UnitAnimationState::Build)
                .map(|clip| (clip.frames.len(), clip.fps, clip.looping)),
            Some((8, 9.0, true))
        );
        assert!(matches!(
            game.unit_animation_atlas(UnitKind::Engineer, UnitAnimationState::Build),
            Some((_, atlas)) if atlas.columns == 8 && atlas.rows == 1
        ));

        game.placing_beacon = false;
        assert_eq!(
            game.unit_animation_state(surveyor, UnitKind::Surveyor, true, Vec2::X * 20.0, false),
            UnitAnimationState::Move
        );
        assert_eq!(
            game.unit_animation_clip(UnitKind::Surveyor, UnitAnimationState::Move)
                .map(|clip| clip.frames.len()),
            Some(6)
        );
        assert!(matches!(
            game.unit_animation_atlas(UnitKind::Surveyor, UnitAnimationState::Move),
            Some((_, atlas)) if atlas.columns == 6 && atlas.rows == 1
        ));

        game.mark_flash.insert(surveyor, 1.0);
        assert_eq!(
            game.unit_animation_state(surveyor, UnitKind::Surveyor, true, Vec2::ZERO, false),
            UnitAnimationState::Mark
        );
        assert_eq!(
            game.unit_animation_clip(UnitKind::Surveyor, UnitAnimationState::Mark)
                .map(|clip| (clip.frames.len(), clip.fps, clip.looping)),
            Some((4, 8.0, true))
        );
        assert!(matches!(
            game.unit_animation_atlas(UnitKind::Surveyor, UnitAnimationState::Mark),
            Some((_, atlas)) if atlas.columns == 4 && atlas.rows == 1
        ));
        assert_eq!(
            game.unit_animation_state(engineer, UnitKind::Engineer, true, Vec2::ZERO, false),
            UnitAnimationState::Idle
        );
    }

    #[test]
    fn ctrl_role_selection_collects_the_live_roster() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let clicked = game
            .simulation
            .world
            .units()
            .iter()
            .find(|unit| game.simulation.kinds.get(&unit.id) == Some(&UnitKind::Warden))
            .map(|unit| unit.id)
            .expect("reclaim starts with a Warden");
        game.simulation.spawn(
            UnitKind::Warden,
            PLAYER,
            Vec2::new(-700.0, -500.0),
            155.0,
            175.0,
            SimulationModifiers::default(),
        );

        assert!(game.select_all_player_units_of_kind(clicked, false));
        let selected = game.simulation.world.selection().ids();
        assert_eq!(selected.len(), 2);
        assert!(selected
            .iter()
            .all(|id| game.simulation.kinds.get(id) == Some(&UnitKind::Warden)));
    }

    #[test]
    fn environment_plate_covers_map_without_stretching_source_art() {
        let size = LastLight::environment_sprite_size();
        let (width, height) = TextureAsset::ReactorSector.spec().pixel_size;
        let source_ratio = width as f32 / height as f32;
        let rendered_ratio = size.x / size.y;
        assert!((rendered_ratio - source_ratio).abs() < 1e-5);
        assert!(size.x >= MAP_SIZE.x);
        assert!(size.y >= MAP_SIZE.y);
    }

    #[test]
    fn selected_unit_terrain_chip_uses_the_authored_engine_readout() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let high_ground = game
            .mission
            .terrain_zones
            .iter()
            .find(|zone| zone.elevation > 0)
            .expect("reclaim authors a high-ground zone");

        let (copy, _) = game
            .terrain_readout_copy(high_ground.bounds.center())
            .expect("terrain resolver should find the authored ridge");
        assert!(copy.starts_with("TERRAIN HIGH // COVER "));
        assert!(copy.ends_with('%'));
        assert!(copy.len() <= 32);

        let (open_copy, _) = game
            .terrain_readout_copy(Vec2::new(1_700.0, 1_000.0))
            .expect("outside authored bands should read as open ground");
        assert_eq!(open_copy, "TERRAIN OPEN // COVER 00%");
    }

    #[test]
    fn alliance_modules_require_their_campaign_decisions() {
        let mut game = LastLight::new();
        assert_eq!(game.meridian_accord(), None);
        assert_eq!(game.verdant_covenant(), None);

        game.save_data.campaign.record_decision(MERIDIAN_ALLIED);
        game.save_data.campaign.record_decision(VERDANT_CULTIVATED);
        assert_eq!(game.meridian_accord(), Some(MERIDIAN_BASTION));
        assert_eq!(game.verdant_covenant(), Some(VERDANT_BLOOM));

        game.save_data
            .campaign
            .equip_specialist(MERIDIAN, MERIDIAN_CHARTER);
        game.save_data
            .campaign
            .equip_specialist(VERDANT, VERDANT_BRIAR);
        assert_eq!(game.meridian_accord(), Some(MERIDIAN_CHARTER));
        assert_eq!(game.verdant_covenant(), Some(VERDANT_BRIAR));
    }

    #[test]
    fn salvage_charter_stacks_with_existing_economy_loadouts() {
        let mut game = LastLight::new();
        assert_eq!(game.beacon_cost(), 50);
        assert_eq!(game.relay_income(), 4);

        game.save_data.campaign.record_decision(MERIDIAN_ALLIED);
        game.save_data
            .campaign
            .equip_specialist(MERIDIAN, MERIDIAN_CHARTER);
        game.save_data.campaign.equip_specialist(IVO, IVO_SMITH);
        assert_eq!(game.beacon_cost(), 30);
        assert_eq!(game.relay_income(), 5);
    }

    #[test]
    fn mission_select_hides_locked_missions_until_unlocked() {
        let mut game = LastLight::new();
        assert_eq!(game.unlocked_mission_indices(), vec![0]);
        game.save_data.campaign.unlocked_mission = 3;
        assert_eq!(game.unlocked_mission_indices(), vec![0, 1]);
        game.save_data.campaign.unlocked_mission = 5;
        assert_eq!(game.unlocked_mission_indices(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn garden_below_adds_a_verdant_escort_chapter() {
        let mut game = LastLight::new();
        let garden = missions::garden_below();
        assert_eq!(garden.required_tier, 5);
        assert_eq!(garden.unlock_decision, Some("verdant-cultivated"));
        assert!(garden.salvage_nodes.len() >= 6);
        assert!(garden.terrain_zones.len() >= 4);
        assert!(matches!(
            garden.victory,
            VictoryCondition::EscortToExtraction { .. }
        ));

        game.start_mission(garden);
        assert!(game.simulation.escort_unit.is_some());
        assert_eq!(game.simulation.relays.len(), 3);
        assert_eq!(game.simulation.identities.len(), 3);
    }

    #[test]
    fn starting_reclaim_the_reactor_populates_relays_and_roster() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        assert_eq!(game.simulation.relays.len(), 3);
        assert_eq!(game.salvage_nodes.len(), 5);
        assert!(game.mission.terrain_zones.len() >= 3);
        assert!(game.reactor_position.is_some());
        assert_eq!(
            game.simulation
                .world
                .units()
                .iter()
                .filter(|unit| unit.faction == PLAYER && unit.alive())
                .count(),
            3
        );
        let named_units: Vec<_> = game.simulation.identities.values().copied().collect();
        assert!(named_units.contains(&"MARA VEY"));
        assert!(named_units.contains(&"IVO ROOK"));
        assert!(named_units.contains(&"SENA QUILL"));
        assert_eq!(game.simulation.world.selection().ids().len(), 3);
        assert_eq!(
            game.simulation
                .world
                .units()
                .iter()
                .filter(|unit| unit.faction == CHOIR)
                .count(),
            6
        );
    }

    #[test]
    fn idle_warden_acquires_a_visible_enemy() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let warden = game
            .simulation
            .kinds
            .iter()
            .find(|(_, kind)| **kind == UnitKind::Warden)
            .map(|(id, _)| *id)
            .unwrap();
        let enemy = game
            .simulation
            .world
            .units()
            .iter()
            .find(|unit| unit.faction == CHOIR)
            .map(|unit| unit.id)
            .unwrap();
        let warden_position = game.simulation.world.unit(warden).unwrap().position;
        game.simulation.world.unit_mut(enemy).unwrap().position =
            warden_position + Vec2::new(100.0, 0.0);
        game.update_fog();

        game.update_auto_targeting();

        assert_eq!(
            game.simulation.world.unit(warden).unwrap().order,
            UnitOrder::Attack(enemy)
        );
    }

    #[test]
    fn opening_relay_push_has_time_to_read_the_first_raid() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let relay = game.simulation.relays[0].position;
        game.simulation.world.issue_move(relay, 70.0);
        game.update_fog();

        let dt = 1.0 / 60.0;
        for _ in 0..(80 * 60) {
            game.update_enemy_ai(dt);
            game.update_auto_targeting();
            let modifiers = game.simulation_modifiers();
            game.simulation.set_combat_scales(
                modifiers.player_damage_scale,
                modifiers.player_damage_taken_scale,
            );
            game.simulation.fixed_step_with_dt(dt);
            game.update_fog();
            game.mission_time += dt;
        }

        assert_ne!(
            game.simulation.outcome,
            MissionOutcome::Defeat,
            "the opening relay order should not silently wipe the starting roster"
        );
    }

    #[test]
    fn raid_hud_copy_surfaces_only_the_actionable_warning_window() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        assert_eq!(
            LastLight::raid_hud_copy(game.simulation.raid_state()),
            None,
            "the teaching phase should keep the tactical strip quiet"
        );

        game.simulation.enemy_resources.primary = 90;
        let teaching = game.simulation.raid_state();
        game.simulation
            .fixed_step_with_dt(teaching.seconds_remaining - simulation::RAID_WARNING_WINDOW);
        let state = game.simulation.raid_state();
        assert_eq!(state.phase, RaidPhase::Warning);
        assert_eq!(
            LastLight::raid_hud_copy(state).as_deref(),
            Some("RAID 01 // NEEDLE IN 08s")
        );
    }

    #[test]
    fn terminal_overlay_actions_retry_or_return_to_mission_select() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.defeat = true;
        game.simulation.outcome = MissionOutcome::Defeat;
        game.handle_terminal_input(KeyCode::Space);
        assert!(game.briefing);
        assert!(!game.defeat);
        assert_eq!(game.simulation.resources.amount(), 150);

        game.victory = true;
        game.simulation.outcome = MissionOutcome::Victory;
        game.handle_terminal_input(KeyCode::Escape);
        assert!(game.mission_select);
        assert!(!game.victory);
        assert!(!game.defeat);
    }

    #[test]
    fn surveyor_carries_finite_salvage_back_to_the_fabricator() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let surveyor = game
            .simulation
            .kinds
            .iter()
            .find(|(_, kind)| **kind == UnitKind::Surveyor)
            .map(|(id, _)| *id)
            .unwrap();
        let node_position = game.salvage_nodes[0].position;
        game.simulation.world.unit_mut(surveyor).unwrap().position = node_position;
        let salvage_before = game.simulation.resources.amount();

        assert_eq!(game.assign_harvest_order(0), 1);
        assert_eq!(game.update_harvesting(0.0), 0);
        assert_eq!(game.update_harvesting(2.0), 0);
        assert_eq!(game.simulation.resources.amount(), salvage_before);
        assert_eq!(game.salvage_nodes[0].remaining, 216);
        assert_eq!(game.harvest_jobs[&surveyor].cargo, 24);

        game.salvage_nodes[0].remaining = 0;
        game.simulation.world.unit_mut(surveyor).unwrap().position = game.fabricator_position;
        assert_eq!(game.update_harvesting(0.0), 24);
        assert_eq!(game.simulation.resources.amount(), salvage_before + 24);

        // A completed pocket should not strand the worker. The route keeps
        // its depot return, then selects the nearest unsaturated node.
        assert_eq!(game.harvest_jobs[&surveyor].node, 3);
        assert_eq!(game.harvest_jobs[&surveyor].phase, HarvestPhase::ToNode);
    }

    #[test]
    fn structures_expose_distinct_resource_backed_commands() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        for relay in &mut game.simulation.relays {
            relay.active = true;
        }
        game.simulation.resources.credit(300);
        let relay_position = game.simulation.relays[0].position;
        let warden = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .unwrap();
        {
            let unit = game.simulation.world.unit_mut(warden).unwrap();
            unit.position = relay_position;
            unit.health = 50.0;
        }
        let before_pulse = game.simulation.resources.amount();
        game.activate_structure_command(StructureKind::Relay(0));
        assert_eq!(game.simulation.resources.amount(), before_pulse - 35);
        assert_eq!(game.simulation.world.unit(warden).unwrap().health, 85.0);

        let before_core = game.simulation.resources.amount();
        game.activate_structure_command(StructureKind::Reactor);
        assert_eq!(game.simulation.resources.amount(), before_core - 90);
        assert_eq!(game.lumen_cores, 1);
    }

    #[test]
    fn structure_cards_keep_status_copy_inside_the_compact_width_budget() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());

        let relay = game.structure_status_line(StructureKind::Relay(0));
        assert!(relay.starts_with("RELAY 1 // CHARGING"));
        assert!(relay.contains("HP"));
        assert!(relay.chars().count() <= 48);

        game.simulation.relays[0].active = true;
        let online_relay = game.structure_status_line(StructureKind::Relay(0));
        assert!(online_relay.starts_with("RELAY 1 // ONLINE"));
        assert!(online_relay.chars().count() <= 48);

        let fabricator = game.structure_status_line(StructureKind::Fabricator);
        assert!(fabricator.starts_with("FABRICATOR //"));
        assert!(fabricator.contains("Q0/5"));
        assert!(fabricator.contains("M0/3"));
        assert!(fabricator.contains("NO-RALLY"));
        assert!(fabricator.chars().count() <= 56);

        let reactor = game.structure_status_line(StructureKind::Reactor);
        assert!(reactor.starts_with("REACTOR //"));
        assert!(reactor.contains("LATTICE"));
        assert!(reactor.chars().count() <= 48);
    }

    #[test]
    fn structure_visual_state_reflects_boot_power_damage_and_module_builds() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());

        assert_eq!(
            game.structure_visual_state(StructureKind::Relay(0)),
            Some(StructureVisualState::Booting)
        );
        assert_eq!(
            game.structure_visual_state(StructureKind::Reactor),
            Some(StructureVisualState::Booting)
        );
        assert_eq!(
            game.structure_visual_state(StructureKind::Fabricator),
            Some(StructureVisualState::Online)
        );

        let relay = game
            .simulation
            .structures
            .get_mut(&StructureKind::Relay(0))
            .expect("reclaim authors relay structures");
        relay.build_progress = 1.0;
        relay.powered = true;
        assert_eq!(
            game.structure_visual_state(StructureKind::Relay(0)),
            Some(StructureVisualState::Online)
        );

        {
            let fabricator = game
                .simulation
                .structures
                .get_mut(&StructureKind::Fabricator)
                .expect("reclaim authors a fabricator");
            fabricator.powered = false;
        }
        assert_eq!(
            game.structure_visual_state(StructureKind::Fabricator),
            Some(StructureVisualState::Offline)
        );
        {
            let fabricator = game
                .simulation
                .structures
                .get_mut(&StructureKind::Fabricator)
                .expect("reclaim authors a fabricator");
            fabricator.powered = true;
        }
        game.simulation.supply_module_progress = Some(3.0);
        assert_eq!(
            game.structure_visual_state(StructureKind::Fabricator),
            Some(StructureVisualState::Booting)
        );
        game.simulation.supply_module_progress = None;
        {
            let fabricator = game
                .simulation
                .structures
                .get_mut(&StructureKind::Fabricator)
                .expect("reclaim authors a fabricator");
            fabricator.health = fabricator.max_health - 1.0;
        }
        assert_eq!(
            game.structure_visual_state(StructureKind::Fabricator),
            Some(StructureVisualState::Damaged)
        );
    }

    #[test]
    fn fabricator_card_surfaces_queue_rally_and_next_action_in_place() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.selected_structure = Some(StructureKind::Fabricator);

        assert_eq!(game.fabricator_card_title(), "FABRICATOR // Q/E/F QUEUE");
        let idle = game.structure_status_line(StructureKind::Fabricator);
        assert!(idle.contains("Q0/5"));
        assert!(idle.contains("NO-RALLY"));
        assert!(idle.contains("M0/3"));

        game.simulation.set_rally_point(Vec2::new(420.0, 360.0));
        let rallied = game.structure_status_line(StructureKind::Fabricator);
        assert!(rallied.contains("RALLY"));
        assert!(!rallied.contains("NO-RALLY"));
        assert!(rallied.chars().count() <= 56);
    }

    #[test]
    fn fabricator_card_exposes_cancel_refund_only_when_queue_has_work() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.selected_structure = Some(StructureKind::Fabricator);

        assert_eq!(game.command_card_key(4), Some(KeyCode::KeyT));
        assert!(!game.command_card_available(4));

        game.simulation.queue_unit(UnitKind::Warden).unwrap();
        assert_eq!(game.command_card_key(4), Some(KeyCode::KeyX));
        assert!(game.command_card_display(4).contains("75% REFUND"));
        assert!(game.command_card_available(4));

        game.cancel_queued_unit(0);
        assert!(game.simulation.production.items().is_empty());
        assert!(game
            .status
            .as_ref()
            .is_some_and(|(text, _)| text.contains("WARDEN CANCELLED")));
        assert_eq!(game.command_card_key(4), Some(KeyCode::KeyT));
    }

    #[test]
    fn fabricator_build_rows_explain_contextual_admission_blockers() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.selected_structure = Some(StructureKind::Fabricator);

        assert_eq!(game.command_card_display(0), "Q  WARDEN  90");
        assert!(game.command_card_available(0));

        game.simulation.power.set_online(FABRICATOR_NODE, false);
        assert_eq!(game.command_card_display(0), "Q  WARDEN // OFFLINE");
        assert!(!game.command_card_available(0));

        game.simulation.power.set_online(FABRICATOR_NODE, true);
        game.simulation.flux = 0;
        assert_eq!(game.command_card_display(2), "F  SURVEYOR // NEED 1 FLUX");
        assert!(!game.command_card_available(2));

        game.simulation.flux = 3;
        let salvage = game.simulation.resources.amount();
        let _ = game.simulation.resources.spend(salvage);
        assert_eq!(game.command_card_display(0), "Q  WARDEN // SALV 90");
        assert!(!game.command_card_available(0));

        game.simulation.resources.credit(500);
        game.simulation.supply.set_capacity(0);
        assert_eq!(game.command_card_display(0), "Q  WARDEN // SUPPLY FULL");
        assert!(!game.command_card_available(0));

        game.simulation.supply.set_capacity(12);
        for _ in 0..5 {
            game.simulation.queue_unit(UnitKind::Warden).unwrap();
        }
        assert_eq!(game.command_card_display(0), "Q  WARDEN // QUEUE FULL");
        assert!(!game.command_card_available(0));
    }

    #[test]
    fn disabled_fabricator_command_row_cannot_queue_or_spend() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.selected_structure = Some(StructureKind::Fabricator);

        game.simulation.power.set_online(FABRICATOR_NODE, false);
        assert!(!game.command_card_available(0));

        let before_status = game.status.clone();
        let before_queue = game.simulation.production.items().len();
        let before_resource = game.simulation.resources.amount();
        let before_flux = game.simulation.flux;

        game.apply_command_action(KeyCode::KeyQ);

        assert_eq!(game.status, before_status);
        assert_eq!(game.simulation.production.items().len(), before_queue);
        assert_eq!(game.simulation.resources.amount(), before_resource);
        assert_eq!(game.simulation.flux, before_flux);
    }

    #[test]
    fn fabricator_title_reports_front_build_progress_and_offline_cancel() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.selected_structure = Some(StructureKind::Fabricator);

        assert_eq!(game.fabricator_card_title(), "FABRICATOR // Q/E/F QUEUE");
        game.simulation.queue_unit(UnitKind::Warden).unwrap();
        let initial_progress = game
            .simulation
            .production
            .items()
            .front()
            .expect("queued Warden")
            .progress();
        assert_eq!(
            game.fabricator_card_title(),
            format!(
                "FABRICATOR // BUILD WARDEN {:02}% // Q1/5",
                (initial_progress * 100.0).round() as u32
            )
        );

        game.simulation.fixed_step_with_dt(1.0);
        let progress = game
            .simulation
            .production
            .items()
            .front()
            .expect("Warden remains in front after one second")
            .progress();
        assert!(progress > initial_progress);
        assert_eq!(
            game.fabricator_card_title(),
            format!(
                "FABRICATOR // BUILD WARDEN {:02}% // Q1/5",
                (progress * 100.0).round() as u32
            )
        );

        game.simulation.power.set_online(FABRICATOR_NODE, false);
        assert_eq!(game.command_card_display(4), "X  CANCEL // OFFLINE");
        assert!(!game.command_card_available(4));
        assert!(game.fabricator_card_title().contains("OFFLINE"));
    }

    #[test]
    fn fabricator_module_row_explains_each_build_gate() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.selected_structure = Some(StructureKind::Fabricator);

        assert_eq!(game.fabricator_module_copy(), "D MOD 100");
        let ready_row = game.command_card_display(5);
        assert!(ready_row.starts_with("B BEACON 50  D MOD 100"));
        assert_eq!(ready_row.matches("D MOD").count(), 1);

        game.selected_structure = None;
        game.simulation.world.clear_selection();
        assert_eq!(game.command_card_label(5), "B BEACON 50");
        game.selected_structure = Some(StructureKind::Fabricator);

        game.simulation.power.set_online(FABRICATOR_NODE, false);
        assert_eq!(game.fabricator_module_copy(), "D MOD // OFFLINE");

        game.simulation.power.set_online(FABRICATOR_NODE, true);
        let _ = game
            .simulation
            .resources
            .spend(game.simulation.resources.amount());
        assert_eq!(game.fabricator_module_copy(), "D MOD // 100");

        game.simulation.resources.credit(100);
        game.upgrade_supply_module();
        assert_eq!(game.fabricator_module_copy(), "D MOD // 00%");

        game.simulation.fixed_step_with_dt(1.0);
        assert_eq!(game.fabricator_module_copy(), "D MOD // 17%");

        game.simulation.supply_module_progress = None;
        game.simulation.supply_module_level = 3;
        assert_eq!(game.fabricator_module_copy(), "D MOD // MAXED");
    }

    #[test]
    fn structure_command_cards_explain_locked_actions_before_click() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());

        game.selected_structure = Some(StructureKind::Relay(0));
        assert_eq!(
            game.command_card_display(0),
            "C PULSE // ENGINEER",
            "an offline relay should teach the required specialist"
        );
        assert!(!game.command_card_available(0));

        game.simulation.relays[0].active = true;
        let _ = game.simulation.resources.spend(120);
        assert_eq!(game.command_card_display(0), "C PULSE // NEED 35");
        assert!(!game.command_card_available(0));

        game.selected_structure = Some(StructureKind::Reactor);
        assert_eq!(game.command_card_display(0), "C CORE // RESTORE RELAYS");
        assert!(!game.command_card_available(0));

        for relay in &mut game.simulation.relays {
            relay.active = true;
        }
        assert_eq!(game.command_card_display(0), "C CORE // NEED 90");
        assert!(!game.command_card_available(0));
    }

    #[test]
    fn fabricator_rally_routes_new_units_to_the_front() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let rally = Vec2::new(420.0, 360.0);
        game.simulation.set_rally_point(rally);
        game.simulation.queue_unit(UnitKind::Warden).unwrap();
        game.simulation.fixed_step_with_dt(8.0);
        let deployed = game
            .simulation
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == PLAYER && unit.position.distance(rally) < 130.0)
            .count();
        assert!(
            deployed >= 1,
            "queued units should deploy at the rally point"
        );
    }

    #[test]
    fn fabricator_resource_rally_routes_new_surveyor_and_preserves_one_job() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let surveyor = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .unwrap();
        let node_position = game.salvage_nodes[0].position;
        game.simulation
            .set_rally_point(node_position + Vec2::new(24.0, 0.0));

        assert_eq!(
            game.apply_surveyor_rally(surveyor),
            RallyHarvestOutcome::Assigned(0)
        );
        assert_eq!(game.harvest_jobs.len(), 1);
        assert_eq!(game.harvest_jobs[&surveyor].node, 0);
        assert_eq!(game.harvest_jobs[&surveyor].phase, HarvestPhase::ToNode);
        assert!(game
            .status
            .as_ref()
            .is_some_and(|(text, _)| text.contains("SALVAGE NODE 1")));
        assert_eq!(
            game.radio_message.map(|(speaker, _, _)| speaker),
            Some("SENA QUILL")
        );

        // A duplicate deployment event must preserve the existing route,
        // rather than removing and recreating a second logical job.
        assert_eq!(
            game.apply_surveyor_rally(surveyor),
            RallyHarvestOutcome::AlreadyAssigned(0)
        );
        assert_eq!(game.harvest_jobs.len(), 1);
    }

    #[test]
    fn fabricator_normal_rally_does_not_create_a_harvest_job() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let surveyor = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .unwrap();
        let rally = game.fabricator_position + Vec2::new(0.0, 260.0);
        assert!(game.salvage_node_at(rally).is_none());
        game.simulation.set_rally_point(rally);
        game.status = None;

        assert_eq!(
            game.apply_surveyor_rally(surveyor),
            RallyHarvestOutcome::NotResource
        );
        assert!(game.harvest_jobs.is_empty());
        assert!(game.status.is_none());
    }

    #[test]
    fn fabricator_rally_to_depleted_node_reports_dry_without_harvest_job() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let surveyor = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .unwrap();
        let node_position = game.salvage_nodes[0].position;
        game.salvage_nodes[0].remaining = 0;
        game.simulation.set_rally_point(node_position);
        game.status = None;

        assert_eq!(
            game.apply_surveyor_rally(surveyor),
            RallyHarvestOutcome::Dry(0)
        );
        assert!(game.harvest_jobs.is_empty());
        assert!(game
            .status
            .as_ref()
            .is_some_and(|(text, _)| text.contains("NODE 1 DRY")));
    }

    #[test]
    fn fabricator_rally_to_saturated_node_holds_extra_surveyor_at_rally() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let first = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .unwrap();
        let modifiers = game.simulation_modifiers();
        let second = game.simulation.spawn(
            UnitKind::Surveyor,
            PLAYER,
            Vec2::new(-640.0, -420.0),
            90.0,
            210.0,
            modifiers,
        );
        let third = game.simulation.spawn(
            UnitKind::Surveyor,
            PLAYER,
            Vec2::new(-580.0, -420.0),
            90.0,
            210.0,
            modifiers,
        );
        assert_eq!(game.assign_surveyors_to_node(0, &[first, second]), 2);
        assert_eq!(game.workers_at_node(0), 2);

        game.simulation
            .set_rally_point(game.salvage_nodes[0].position);
        game.status = None;
        assert_eq!(
            game.apply_surveyor_rally(third),
            RallyHarvestOutcome::Saturated(0)
        );
        assert_eq!(game.workers_at_node(0), 2);
        assert!(!game.harvest_jobs.contains_key(&third));
        assert!(game
            .status
            .as_ref()
            .is_some_and(|(text, _)| text.contains("SATURATED")));
    }

    #[test]
    fn fabricator_supply_menu_queues_capacity_before_completion() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let before = game.simulation.supply.capacity();
        game.upgrade_supply_module();
        assert_eq!(game.simulation.supply.capacity(), before);
        assert!(game.simulation.supply_module_progress.is_some());

        game.simulation.fixed_step_with_dt(6.0);
        assert_eq!(game.simulation.supply.capacity(), before + 4);
        assert_eq!(game.simulation.supply_module_level, 1);
    }

    #[test]
    fn campaign_radio_lines_trigger_in_authored_order() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.mission_time = 2.0;

        game.update_radio_dialogue(0.0);

        assert_eq!(
            game.radio_message.map(|(speaker, _, _)| speaker),
            Some("MARA VEY")
        );
        assert_eq!(game.dialogue_cursor, 1);
    }

    #[test]
    fn garden_resource_completion_queues_authored_radio_line() {
        let mut game = LastLight::new();
        game.start_mission(missions::garden_below());
        let line_index = game
            .mission
            .radio_lines
            .iter()
            .position(|line| matches!(line.trigger, DialogueTrigger::ResourceObjectiveCompleted))
            .expect("garden authors a resource completion transmission");
        let (objective, target) = game
            .simulation
            .resource_objective_contract()
            .expect("garden resource objective resolves to a node");

        for unit in game.simulation.world.units_mut() {
            if unit.faction == CHOIR {
                unit.position = Vec2::new(1_600.0, 900.0);
            }
        }
        for (id, kind) in game.simulation.kinds.clone() {
            if let Some(unit) = game.simulation.world.unit_mut(id) {
                match kind {
                    UnitKind::Surveyor | UnitKind::Warden => unit.position = target,
                    _ => {}
                }
            }
        }
        game.simulation
            .fixed_step_with_dt(objective.required_seconds);
        assert!(game
            .simulation
            .resource_objective_state()
            .is_some_and(|state| state.completed));

        game.dialogue_cursor = line_index;
        game.radio_message = None;
        game.radio_queue.clear();
        game.radio_priority_queue.clear();
        game.update_radio_dialogue(0.0);

        let (speaker, text, _) = game
            .radio_message
            .expect("completed resource objective should enter the radio queue");
        assert_eq!(speaker, "SENA QUILL");
        assert!(text.contains("CACHE IS SECURE"));
        assert_eq!(game.dialogue_cursor, line_index + 1);
    }

    #[test]
    fn objective_resolution_tracks_mission_progression() {
        let mut reclaim = LastLight::new();
        reclaim.start_mission(missions::reclaim_the_reactor());
        assert_eq!(
            reclaim.next_objective().map(|(position, _)| position),
            Some(reclaim.simulation.relays[0].position)
        );
        for relay in &mut reclaim.simulation.relays {
            relay.active = true;
        }
        let canticle = reclaim
            .simulation
            .world
            .units()
            .iter()
            .find(|unit| reclaim.simulation.kinds.get(&unit.id) == Some(&UnitKind::Canticle))
            .expect("reclaim mission includes the Canticle");
        assert_eq!(
            reclaim.next_objective().map(|(position, _)| position),
            Some(canticle.position)
        );

        let mut voice = LastLight::new();
        voice.start_mission(missions::voice_in_conduit_twelve());
        assert_eq!(
            voice.next_objective().map(|(_, label)| label),
            Some("SURVEYOR SCAN // HOLD 00%".to_owned())
        );
        voice.specialist_objective_state.completed = true;
        assert_eq!(
            voice.next_objective().map(|(_, label)| label),
            Some("ESCORT SENA TO THE ARRAY".to_owned())
        );
    }

    #[test]
    fn garden_objective_priority_falls_through_to_resource_before_extraction() {
        let mut game = LastLight::new();
        game.start_mission(missions::garden_below());
        let repair = game
            .mission
            .engineer_repair_objective
            .expect("garden authors an Engineer repair beat");
        let (resource, resource_target) = game
            .simulation
            .resource_objective_contract()
            .expect("garden authors a resource beat");

        assert_eq!(
            game.next_objective(),
            Some((repair.target, "ENGINEER REPAIR // HOLD 00%".to_owned()))
        );

        // Completing the role-specific beat should reveal the authored node,
        // not jump straight to the generic escort victory condition.
        game.specialist_objective_state.completed = true;
        assert_eq!(
            game.next_objective(),
            Some((
                resource_target,
                "RESOURCE // NODE 2 // SEND SURVEYOR 00%".to_owned()
            ))
        );

        // Isolate the authored pocket so the deterministic fixed step can
        // complete the resource contract without a combat contest.
        for unit in game.simulation.world.units_mut() {
            if unit.faction == CHOIR {
                unit.position = Vec2::new(1_600.0, 900.0);
            }
        }
        let surveyor = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .expect("garden includes a Surveyor");
        game.simulation.world.unit_mut(surveyor).unwrap().position = resource_target;
        game.simulation
            .fixed_step_with_dt(resource.required_seconds);
        assert!(game
            .simulation
            .resource_objective_state()
            .is_some_and(|state| state.completed));
        assert_eq!(
            game.next_objective().map(|(_, label)| label),
            Some("ESCORT SENA TO THE ARRAY".to_owned())
        );
    }

    #[test]
    fn garden_resource_objective_provides_the_r_focus_target_and_copy() {
        let mut game = LastLight::new();
        game.start_mission(missions::garden_below());
        game.specialist_objective_state.completed = true;

        let (chip_target, chip_copy, _) = game
            .resource_objective_hud_copy()
            .expect("garden resource objective exposes a HUD chip");
        let (focus_target, focus_label) = game
            .next_objective()
            .expect("R focus should have an active authored objective");

        // KeyR delegates to `focus_next_objective`, which consumes this same
        // tuple; matching the target and copy prevents a stale beacon or
        // camera jump when a resource objective becomes active.
        assert_eq!(focus_target, chip_target);
        assert_eq!(focus_label, format!("RESOURCE // {chip_copy}"));
        assert!(focus_label.contains("NODE 2"));
        assert!(focus_label.contains("SEND SURVEYOR"));
    }

    #[test]
    fn resource_objective_chip_tracks_waiting_progress_contest_and_completion() {
        let mut game = LastLight::new();
        game.start_mission(missions::garden_below());

        let (objective, target) = game
            .simulation
            .resource_objective_contract()
            .expect("garden authors a resource objective");
        let (_, waiting, _) = game
            .resource_objective_hud_copy()
            .expect("active objective has a HUD chip");
        assert_eq!(waiting, "NODE 2 // SEND SURVEYOR 00%");
        assert!(waiting.chars().count() <= 32);

        let surveyor = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .expect("garden includes a Surveyor");
        let choir_ids: Vec<UnitId> = game
            .simulation
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == CHOIR)
            .map(|unit| unit.id)
            .collect();
        for id in choir_ids {
            game.simulation.world.unit_mut(id).unwrap().position = Vec2::new(1_500.0, 900.0);
        }
        game.simulation.world.unit_mut(surveyor).unwrap().position = target;
        game.simulation.fixed_step_with_dt(2.0);
        let (_, securing, _) = game.resource_objective_hud_copy().unwrap();
        assert_eq!(securing, "NODE 2 // SECURING 25%");
        assert!(securing.chars().count() <= 32);

        let needle = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Needle).then_some(*id))
            .expect("garden includes a contesting Needle");
        game.simulation.world.unit_mut(needle).unwrap().position = target;
        game.simulation.fixed_step_with_dt(1.0);
        let (_, contested, _) = game.resource_objective_hud_copy().unwrap();
        assert_eq!(contested, "NODE 2 // CONTESTED 25%");
        assert!(contested.chars().count() <= 32);

        let warden = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("garden includes a Warden support unit");
        game.simulation.world.unit_mut(warden).unwrap().position = target;
        game.simulation.world.unit_mut(needle).unwrap().position = Vec2::new(1_500.0, 900.0);
        game.simulation
            .fixed_step_with_dt(objective.required_seconds);
        let (_, complete, _) = game.resource_objective_hud_copy().unwrap();
        assert_eq!(complete, "NODE 2 // SECURED");
        assert!(complete.chars().count() <= 32);

        let mut reclaim = LastLight::new();
        reclaim.start_mission(missions::reclaim_the_reactor());
        assert!(reclaim.resource_objective_hud_copy().is_none());
    }

    #[test]
    fn specialist_objective_progress_copy_survives_completion_handoff() {
        let mut game = LastLight::new();
        game.start_mission(missions::voice_in_conduit_twelve());

        assert_eq!(
            game.specialist_objective_progress_line().as_deref(),
            Some("SCAN ARRAY 00%")
        );
        game.specialist_objective_state.completed = true;
        assert_eq!(
            game.specialist_objective_progress_line().as_deref(),
            Some("SCAN COMPLETE // ESCORT")
        );
    }

    #[test]
    fn warden_hold_objective_uses_role_specific_hud_copy() {
        let mut game = LastLight::new();
        game.start_mission(missions::terms_of_salvage());

        assert_eq!(
            game.specialist_objective_progress_line().as_deref(),
            Some("HOLD RELAY 00%")
        );
        assert_eq!(
            game.next_objective().map(|(_, label)| label),
            Some("WARDEN HOLD // HOLD 00%".to_owned())
        );
        game.specialist_objective_state.completed = true;
        assert_eq!(
            game.specialist_objective_progress_line().as_deref(),
            Some("HOLD COMPLETE // PUSH")
        );
    }

    #[test]
    fn terrain_control_runtime_requires_ridge_and_queues_mara_handoff() {
        let mut game = LastLight::new();
        game.start_mission(missions::terms_of_salvage());
        let objective = game
            .mission
            .terrain_control_objective
            .expect("Terms authors a ridge control beat");
        for unit in game.simulation.world.units_mut() {
            if unit.faction == CHOIR {
                unit.position = Vec2::new(1_600.0, 900.0);
            }
        }
        let warden = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("Terms includes a Warden");
        game.simulation.world.unit_mut(warden).unwrap().position = objective.target;

        assert_eq!(
            game.terrain_control_progress_line().as_deref(),
            Some("RIDGE HOLD 00% // HOLDING")
        );
        for _ in 0..objective.required_seconds.ceil() as usize {
            game.update_terrain_control_objective(1.0);
        }
        assert!(game.terrain_control_state.completed);
        assert_eq!(
            game.terrain_control_progress_line().as_deref(),
            Some("RIDGE SECURED // HIGH GROUND")
        );
        assert_eq!(
            game.radio_message.map(|(speaker, _, _)| speaker),
            Some("MARA VEY")
        );
        assert_eq!(
            game.status.as_ref().map(|(copy, _)| copy.as_str()),
            Some("HIGH GROUND SECURED // FIRING ANGLE OPEN")
        );
    }

    #[test]
    fn terrain_control_runtime_stalls_when_ridge_is_contested() {
        let mut game = LastLight::new();
        game.start_mission(missions::terms_of_salvage());
        let objective = game
            .mission
            .terrain_control_objective
            .expect("Terms authors a ridge control beat");
        let warden = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("Terms includes a Warden");
        game.simulation.world.unit_mut(warden).unwrap().position = objective.target;
        let needle = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Needle).then_some(*id))
            .expect("Terms includes a Needle");
        game.simulation.world.unit_mut(needle).unwrap().position = objective.target;

        game.update_terrain_control_objective(2.0);
        assert_eq!(game.terrain_control_state.progress_seconds, 0.0);
        assert!(game.terrain_control_state.contested);
        assert!(game
            .terrain_control_progress_line()
            .is_some_and(|line| line.contains("CONTESTED")));
    }

    #[test]
    fn garden_engineer_repair_objective_uses_role_specific_hud_copy() {
        let mut game = LastLight::new();
        game.start_mission(missions::garden_below());

        assert_eq!(
            game.specialist_objective_progress_line().as_deref(),
            Some("REPAIR REACTOR 00%")
        );
        assert_eq!(
            game.next_objective().map(|(_, label)| label),
            Some("ENGINEER REPAIR // HOLD 00%".to_owned())
        );
        game.specialist_objective_state.completed = true;
        assert_eq!(
            game.specialist_objective_progress_line().as_deref(),
            Some("REPAIR COMPLETE // EXTRACTION")
        );
    }

    #[test]
    fn selected_engineer_reports_relay_restoration_job() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let engineer = game
            .simulation
            .kinds
            .iter()
            .find(|(_, kind)| **kind == UnitKind::Engineer)
            .map(|(id, _)| *id)
            .expect("mission includes an engineer");
        let relay_position = game.simulation.relays[0].position;
        game.simulation.world.unit_mut(engineer).unwrap().position = relay_position;
        game.simulation
            .world
            .select_point(relay_position, PLAYER, false);

        assert_eq!(
            game.engineer_relay_status().as_deref(),
            Some("ENGINEER LINK // RELAY 1 — RESTORING 00%")
        );
    }

    #[test]
    fn selected_squad_accepts_a_terrain_move_order() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let selected = game.simulation.world.units()[0].id;
        let destination = Vec2::new(-560.0, -120.0);
        game.simulation.world.select_point(
            game.simulation.world.unit(selected).unwrap().position,
            PLAYER,
            false,
        );

        game.issue_move_order(destination);

        assert!(matches!(
            game.simulation.world.unit(selected).unwrap().order,
            UnitOrder::Move(target) if target.distance(destination) < 1.0
        ));
        assert_eq!(game.order_marker.map(|(point, _)| point), Some(destination));
    }

    #[test]
    fn reclaim_the_reactor_victory_requires_relays_and_boss_dead() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.evaluate_mission_state();
        assert!(!game.victory);

        for relay in &mut game.simulation.relays {
            relay.active = true;
        }
        game.simulation.fixed_step_with_dt(0.0);
        game.evaluate_mission_state();
        assert!(!game.victory, "boss is still alive");

        let canticle_ids: Vec<_> = game
            .simulation
            .kinds
            .iter()
            .filter(|(_, kind)| **kind == UnitKind::Canticle)
            .map(|(id, _)| *id)
            .collect();
        for id in canticle_ids {
            if let Some(unit) = game.simulation.world.unit_mut(id) {
                unit.health = 0.0;
            }
        }
        game.simulation.fixed_step_with_dt(0.0);
        game.evaluate_mission_state();
        assert!(game.victory);
    }

    #[test]
    fn voice_in_conduit_twelve_tracks_escort_survival_and_extraction() {
        let mut game = LastLight::new();
        game.start_mission(missions::voice_in_conduit_twelve());
        let escort = game
            .simulation
            .escort_unit
            .expect("mission defines an escort spawn");
        game.evaluate_mission_state();
        assert!(!game.victory);
        assert!(!game.defeat);

        let VictoryCondition::EscortToExtraction { point, .. } = game.mission.victory else {
            panic!("expected an escort victory condition");
        };
        game.simulation.world.unit_mut(escort).unwrap().position = point;
        game.simulation.fixed_step_with_dt(0.0);
        game.evaluate_mission_state();
        assert!(game.victory);

        let mut failed = LastLight::new();
        failed.start_mission(missions::voice_in_conduit_twelve());
        let failed_escort = failed.simulation.escort_unit.unwrap();
        failed
            .simulation
            .world
            .unit_mut(failed_escort)
            .unwrap()
            .health = 0.0;
        failed.simulation.fixed_step_with_dt(0.0);
        failed.evaluate_mission_state();
        assert!(failed.defeat);
    }

    #[test]
    fn enemy_ai_routes_around_conduit_obstacles() {
        let mut game = LastLight::new();
        game.start_mission(missions::voice_in_conduit_twelve());
        // The mission's obstacles should mark at least one nav cell blocked.
        let blocked = game.simulation.nav.is_blocked_at(Vec2::new(-500.0, 480.0));
        assert!(blocked, "corridor wall should block its own center cell");
    }

    #[test]
    fn canticle_calls_reinforcements_once_at_half_health() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let canticle_id = game
            .simulation
            .kinds
            .iter()
            .find(|(_, kind)| **kind == UnitKind::Canticle)
            .map(|(id, _)| *id)
            .expect("mission spawns a Canticle");
        let choir_before = game
            .simulation
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == CHOIR)
            .count();

        // Still above half health: no trigger yet.
        game.simulation.fixed_step_with_dt(0.0);
        assert_eq!(
            game.simulation
                .world
                .units()
                .iter()
                .filter(|unit| unit.faction == CHOIR)
                .count(),
            choir_before
        );

        let canticle = game.simulation.world.unit_mut(canticle_id).unwrap();
        canticle.health = canticle.max_health * 0.5;
        game.simulation.fixed_step_with_dt(0.0);
        assert_eq!(
            game.simulation
                .world
                .units()
                .iter()
                .filter(|unit| unit.faction == CHOIR)
                .count(),
            choir_before + 2
        );

        // Fires only once even if health stays low on later ticks.
        game.simulation.fixed_step_with_dt(0.0);
        assert_eq!(
            game.simulation
                .world
                .units()
                .iter()
                .filter(|unit| unit.faction == CHOIR)
                .count(),
            choir_before + 2
        );
    }

    #[test]
    fn player_move_routes_around_conduit_obstacles() {
        let mut game = LastLight::new();
        game.start_mission(missions::voice_in_conduit_twelve());
        let unit_id = game.simulation.world.units()[0].id;
        let start = game.simulation.world.units()[0].position;
        game.simulation.world.select_point(start, PLAYER, false);
        assert!(game.simulation.world.selection().contains(unit_id));

        let destination = Vec2::new(start.x, -200.0);
        assert!(
            game.simulation.nav.segment_blocked(start, destination),
            "test destination should cross a corridor wall"
        );

        game.simulation.world.issue_move(destination, 74.0);
        game.simulation.route_around_obstacles(&[unit_id]);

        let UnitOrder::Move(first_waypoint) = game.simulation.world.unit(unit_id).unwrap().order
        else {
            panic!("expected a Move order toward the first routed waypoint");
        };
        assert_ne!(
            first_waypoint, destination,
            "a blocked destination should route via an intermediate waypoint, not go straight there"
        );
        assert!(game.simulation.player_paths.contains_key(&unit_id));

        // Simulate arrival at the first waypoint and confirm the route continues.
        {
            let unit = game.simulation.world.unit_mut(unit_id).unwrap();
            unit.position = first_waypoint;
            unit.order = UnitOrder::Idle;
        }
        game.simulation.advance_player_paths();
        let order_after = game.simulation.world.unit(unit_id).unwrap().order;
        assert!(
            matches!(order_after, UnitOrder::Move(_)),
            "should advance to the next queued waypoint after arriving at the first"
        );
    }

    #[test]
    fn command_card_becomes_contextual_for_each_lantern_role() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());

        game.simulation.select_player_kind(UnitKind::Warden);
        assert_eq!(game.command_card_key(0), Some(KeyCode::KeyY));
        assert!(game.command_card_label(0).contains("SURGE"));

        game.simulation.select_player_kind(UnitKind::Engineer);
        assert_eq!(game.command_card_key(0), Some(KeyCode::KeyY));
        assert!(game.command_card_label(0).contains("EMERGENCY REPAIR"));

        game.simulation.select_player_kind(UnitKind::Surveyor);
        assert_eq!(game.command_card_key(1), Some(KeyCode::KeyG));
        assert!(game.command_card_label(1).contains("HARVEST"));
    }

    #[test]
    fn stop_command_returns_selected_lanterns_to_idle_and_clears_routes() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.simulation.select_all_player_units();
        let selected = game.simulation.world.selection().ids().to_vec();
        assert!(!selected.is_empty());

        let first = selected[0];
        game.simulation.world.unit_mut(first).unwrap().order =
            UnitOrder::Move(Vec2::new(900.0, 0.0));
        game.simulation
            .world
            .unit_mut(first)
            .unwrap()
            .queued_orders
            .push_back(UnitOrder::Patrol(Vec2::ZERO, Vec2::new(120.0, 0.0)));
        game.simulation.player_paths.insert(first, VecDeque::new());

        game.apply_command_action(KeyCode::KeyT);

        let unit = game.simulation.world.unit(first).unwrap();
        assert_eq!(unit.order, UnitOrder::Idle);
        assert!(unit.queued_orders.is_empty());
        assert!(unit.velocity.abs_diff_eq(Vec2::ZERO, 1e-6));
        assert!(game.simulation.player_paths.is_empty());
        assert!(game
            .status
            .as_ref()
            .is_some_and(|(text, _)| text == "SQUAD ORDERS STOPPED"));
    }

    #[test]
    fn command_card_collapses_when_the_playfield_has_no_selection() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        assert!(
            game.command_card_visible(),
            "the authored opening squad is selected"
        );

        game.simulation.world.clear_selection();
        assert!(!game.command_card_visible());

        game.selected_structure = Some(StructureKind::Fabricator);
        assert!(game.command_card_visible());
        game.selected_structure = None;
        game.selected_resource_node = Some(0);
        assert!(game.command_card_visible());
    }

    #[test]
    fn command_card_rows_compact_by_default_and_togglable() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.simulation.select_player_kind(UnitKind::Warden);
        assert!(game.command_card_compact);
        assert_eq!(
            game.visible_command_card_rows(),
            vec![0, 1, 2],
            "compact mode should show only the highest-priority rows"
        );
        assert!(game.command_card_has_more_rows());

        game.command_card_compact = false;
        assert_eq!(game.visible_command_card_rows(), vec![0, 1, 2]);
        game.command_card_page = 1;
        game.clamp_command_card_page_to_context();
        assert_eq!(
            game.visible_command_card_rows().len(),
            3,
            "non-compact mode still respects pagination with one page visible"
        );
        assert_eq!(game.visible_command_card_rows(), vec![3, 4, 5]);
        assert!(game.command_card_has_more_rows());
    }

    #[test]
    fn structure_context_clears_stale_resource_card_state() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.selected_structure = Some(StructureKind::Fabricator);
        game.selected_resource_node = Some(0);

        game.normalize_selection_context();

        assert_eq!(game.selected_resource_node, None);
        assert_eq!(game.command_card_label(0), "Q  WARDEN  90");
        assert!(game.command_card_visible());
    }

    #[test]
    fn resource_card_keys_route_to_the_selected_node_actions() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.selected_resource_node = Some(0);
        game.selected_structure = None;
        assert_eq!(game.command_card_key(0), Some(KeyCode::KeyG));
        assert_eq!(game.command_card_key(1), Some(KeyCode::KeyR));
        assert_eq!(
            game.command_row_for_key(KeyCode::KeyQ),
            None,
            "resource cards must not fall through to production shortcuts"
        );
        assert_eq!(game.command_row_for_key(KeyCode::KeyG), Some(0));
        assert_eq!(game.command_row_for_key(KeyCode::KeyR), Some(1));
    }

    #[test]
    fn structure_card_key_lookup_filters_context_rows() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.selected_structure = Some(StructureKind::Relay(0));

        assert_eq!(game.command_row_for_key(KeyCode::KeyC), Some(0));
        assert_eq!(game.command_row_for_key(KeyCode::KeyQ), None);
        assert_eq!(game.command_row_for_key(KeyCode::KeyE), None);
        assert_eq!(
            game.command_row_for_key(KeyCode::KeyF),
            None,
            "structure cards must not fall through to squad/production shortcuts"
        );
    }

    #[test]
    fn mixed_squad_card_exposes_orders_instead_of_production() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        assert!(game.selected_squad_active());
        assert_eq!(game.command_card_key(0), Some(KeyCode::KeyA));
        assert!(game.command_card_label(0).contains("ATTACK-MOVE"));
        assert_eq!(game.command_card_key(5), Some(KeyCode::KeyB));
        assert!(!game.command_card_label(0).contains("WARDEN"));
    }

    #[test]
    fn same_kind_squad_card_hides_specialist_singleton_rows() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());

        let modifiers = game.simulation_modifiers();
        let spawn_point = Vec2::new(-120.0, -80.0);
        game.simulation.spawn(
            UnitKind::Warden,
            PLAYER,
            spawn_point,
            90.0,
            210.0,
            modifiers,
        );
        game.simulation.spawn(
            UnitKind::Warden,
            PLAYER,
            spawn_point + Vec2::new(18.0, 0.0),
            90.0,
            210.0,
            modifiers,
        );

        let warden_ids: Vec<UnitId> = game
            .simulation
            .kinds
            .iter()
            .filter_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .collect();
        game.simulation.world.clear_selection();
        for (index, id) in warden_ids.iter().take(2).enumerate() {
            let position = game
                .simulation
                .world
                .unit(*id)
                .map(|unit| unit.position)
                .expect("selected warden should still exist");
            game.simulation
                .world
                .select_point(position, PLAYER, index != 0);
        }

        assert!(
            game.simulation.world.selection().ids().len() > 1,
            "same-kind squad should contain multiple wardens"
        );
        assert_eq!(game.command_card_key(0), Some(KeyCode::KeyA));
        assert_eq!(game.command_card_label(0), "A  ATTACK-MOVE");
        assert_eq!(game.command_card_key(5), Some(KeyCode::KeyB));
        assert_eq!(game.command_card_label(5), "B  FIELD BEACON");
        assert!(!game.command_card_label(0).contains("SURGE"));
    }

    #[test]
    fn same_kind_squad_key_y_does_not_trigger_singleton_ability() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());

        let modifiers = game.simulation_modifiers();
        let spawn_point = Vec2::new(-120.0, -80.0);
        game.simulation.spawn(
            UnitKind::Warden,
            PLAYER,
            spawn_point,
            90.0,
            210.0,
            modifiers,
        );
        game.simulation.spawn(
            UnitKind::Warden,
            PLAYER,
            spawn_point + Vec2::new(18.0, 0.0),
            90.0,
            210.0,
            modifiers,
        );

        let warden_ids: Vec<UnitId> = game
            .simulation
            .kinds
            .iter()
            .filter_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .collect();
        game.simulation.world.clear_selection();
        for (index, id) in warden_ids.iter().take(2).enumerate() {
            let position = game
                .simulation
                .world
                .unit(*id)
                .map(|unit| unit.position)
                .expect("selected warden should still exist");
            game.simulation
                .world
                .select_point(position, PLAYER, index != 0);
        }

        assert_eq!(game.command_row_for_key(KeyCode::KeyY), None);

        let before_status = game.status.clone();
        let before_queue = game.simulation.production.items().len();
        let before_resource = game.simulation.resources.amount();
        let before_flux = game.simulation.flux;
        let before_cooldown = game.simulation.ability_cooldown(warden_ids[0]);

        game.apply_command_action(KeyCode::KeyY);

        assert_eq!(game.command_row_for_key(KeyCode::KeyY), None);
        assert_eq!(game.status, before_status);
        assert_eq!(game.simulation.production.items().len(), before_queue);
        assert_eq!(game.simulation.resources.amount(), before_resource);
        assert_eq!(game.simulation.flux, before_flux);
        assert_eq!(
            game.simulation.ability_cooldown(warden_ids[0]),
            before_cooldown
        );
    }

    #[test]
    fn singleton_squad_still_surfaces_singleton_ability_rows() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());

        assert!(
            game.simulation.select_player_kind(UnitKind::Warden),
            "the mission should have a warden to validate singleton behavior"
        );
        let warden = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Warden).then_some(*id))
            .expect("reclaim has a warden");
        let position = game.simulation.world.unit(warden).unwrap().position;
        game.simulation.world.clear_selection();
        game.simulation.world.select_point(position, PLAYER, false);

        assert_eq!(game.command_card_key(0), Some(KeyCode::KeyY));
        assert!(game.command_card_label(0).contains("SURGE"));
    }

    #[test]
    fn mixed_squad_copy_discloses_split_hint_only_during_onboarding() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());

        game.controls_hint_remaining = 0.0;
        assert_eq!(game.mixed_squad_role_line([1, 1, 1]), "W1  E1  S1");

        game.controls_hint_remaining = 5.0;
        assert_eq!(
            game.mixed_squad_role_line([1, 1, 1]),
            "W1  E1  S1   // CLICK PORTRAIT TO SPLIT"
        );
    }

    #[test]
    fn resource_node_card_exposes_bounded_inspect_and_assign_actions() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.selected_resource_node = Some(0);

        assert_eq!(game.command_card_key(0), Some(KeyCode::KeyG));
        assert!(game.command_card_label(0).contains("ASSIGN SURVEYOR"));
        assert_eq!(game.command_card_key(1), Some(KeyCode::KeyR));
        assert!(game.command_card_label(1).contains("FOCUS NODE"));
        assert_eq!(
            game.resource_node_status_line(0),
            "SALVAGE 240 LEFT // W0/2 // +00/S"
        );

        let surveyor = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .expect("reclaim starts with a Surveyor");
        game.harvest_jobs.insert(
            surveyor,
            HarvestJob {
                node: 0,
                cargo: 8,
                phase: HarvestPhase::Extracting,
            },
        );
        assert_eq!(
            game.resource_node_status_line(0),
            "SALVAGE 240 LEFT // W1/2 // +18/S"
        );
    }

    #[test]
    fn resource_node_menu_assigns_nearest_idle_surveyor() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.selected_resource_node = Some(0);

        game.assign_selected_resource_node();

        assert_eq!(game.harvest_jobs.len(), 1);
        assert_eq!(
            game.harvest_jobs.values().next().map(|job| job.node),
            Some(0)
        );
    }

    #[test]
    fn idle_surveyor_chip_only_surfaces_unrouted_workers() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());

        assert_eq!(
            game.idle_surveyor_hud_copy().as_deref(),
            Some("IDLE SURVEYOR 1 // I FOCUS")
        );

        let surveyor = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .expect("mission includes a Surveyor");
        assert_eq!(game.assign_surveyors_to_node(0, &[surveyor]), 1);
        assert_eq!(game.idle_surveyor_hud_copy(), None);

        game.simulation.world.unit_mut(surveyor).unwrap().health = 0.0;
        game.harvest_jobs.remove(&surveyor);
        assert_eq!(game.idle_surveyor_hud_copy(), None);
    }

    #[test]
    fn command_row_lookup_matches_visible_context_only() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());

        assert_eq!(game.command_row_for_key(KeyCode::KeyC), None);
        game.selected_structure = Some(StructureKind::Relay(0));
        assert_eq!(game.command_row_for_key(KeyCode::KeyC), Some(0));
        assert_eq!(game.command_row_for_key(KeyCode::KeyQ), None);
    }

    #[test]
    fn structure_context_ignores_unit_and_global_commands() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let queue_before = game.simulation.production.items().len();

        game.selected_structure = Some(StructureKind::Relay(0));
        game.selected_resource_node = None;
        game.simulation.world.clear_selection();
        game.attack_move_mode = false;

        game.apply_command_action(KeyCode::KeyA);
        game.apply_command_action(KeyCode::KeyQ);
        game.apply_command_action(KeyCode::KeyG);
        game.apply_command_action(KeyCode::KeyT);

        assert!(!game.attack_move_mode);
        assert_eq!(game.simulation.production.items().len(), queue_before);
    }

    #[test]
    fn resource_context_ignores_unit_only_production_keys() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let queue_before = game.simulation.production.items().len();

        game.selected_structure = None;
        game.selected_resource_node = Some(0);
        game.attack_move_mode = false;

        game.apply_command_action(KeyCode::KeyQ);
        game.apply_command_action(KeyCode::KeyE);
        game.apply_command_action(KeyCode::KeyA);
        game.apply_command_action(KeyCode::KeyT);

        assert!(!game.attack_move_mode);
        assert_eq!(game.simulation.production.items().len(), queue_before);
    }

    #[test]
    fn fabricator_split_row_routes_pointer_to_beacon_or_module() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.selected_structure = Some(StructureKind::Fabricator);
        let card_text = Vec2::ZERO;
        let rect = LastLight::command_card_row_rect(card_text, 2, 1.0);
        assert_eq!(
            game.command_card_key_at(5, 2, rect.center() + Vec2::new(-1.0, 0.0), card_text, 1.0),
            Some(KeyCode::KeyB)
        );
        assert_eq!(
            game.command_card_key_at(5, 2, rect.center() + Vec2::new(1.0, 0.0), card_text, 1.0),
            Some(KeyCode::KeyD)
        );
    }

    #[test]
    fn resource_nodes_report_and_enforce_worker_saturation() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        assert_eq!(game.salvage_nodes[0].max_workers, 2);
        assert_eq!(game.salvage_nodes[3].max_workers, 3);
        assert_eq!(game.workers_at_node(0), 0);

        let surveyor = game
            .simulation
            .kinds
            .iter()
            .find_map(|(id, kind)| (*kind == UnitKind::Surveyor).then_some(*id))
            .unwrap();
        game.simulation.world.select_point(
            game.simulation.world.unit(surveyor).unwrap().position,
            PLAYER,
            false,
        );
        assert_eq!(game.assign_harvest_order(0), 1);
        assert_eq!(game.workers_at_node(0), 1);
    }

    #[test]
    fn radio_inbox_queues_lines_and_keeps_a_transmission_focus() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let relay = game.simulation.relays[0].position;
        game.queue_radio_line("IVO ROOK", "Relay online.", Some(relay));
        game.queue_radio_line("MARA VEY", "Hold the line.", Some(Vec2::ZERO));
        assert_eq!(
            game.radio_message.map(|(speaker, _, _)| speaker),
            Some("IVO ROOK")
        );
        assert_eq!(game.radio_queue.len(), 1);
        assert_eq!(game.last_transmission, Some(Vec2::ZERO));

        game.update_radio_dialogue(6.1);
        assert_eq!(
            game.radio_message.map(|(speaker, _, _)| speaker),
            Some("MARA VEY")
        );
    }

    #[test]
    fn urgent_radio_lines_precede_ambient_inbox() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.queue_radio_line("IVO ROOK", "Ambient relay update.", Some(Vec2::ZERO));
        game.queue_radio_line(
            "SENA QUILL",
            "Ambient queue item.",
            Some(Vec2::new(10.0, 0.0)),
        );
        game.queue_urgent_radio_line("PREFECT VALE", "Raid warning.", Some(Vec2::new(20.0, 0.0)));

        assert_eq!(game.radio_queue.len(), 1);
        assert_eq!(game.radio_priority_queue.len(), 1);

        game.update_radio_dialogue(6.1);
        assert_eq!(
            game.radio_message.map(|(speaker, _, _)| speaker),
            Some("PREFECT VALE")
        );
        assert_eq!(game.radio_queue.len(), 1);
        assert!(game.radio_priority_queue.is_empty());
    }

    #[test]
    fn radio_copy_wraps_inside_the_compact_comms_card() {
        let [first, second] = LastLight::radio_line_chunks(
            "THE GARDEN IS NOT PLANT LIFE. IT IS A MAP THAT LEARNED TO BREATHE.",
        );
        assert!(first.chars().count() <= 54);
        assert!(second.chars().count() <= 54);
        assert!(!second.is_empty());
    }

    #[test]
    fn briefing_copy_wraps_and_preserves_the_authored_speaker() {
        let mut game = LastLight::new();
        game.start_mission(missions::garden_below());
        assert_eq!(game.briefing_speaker(), "SENA QUILL");
        assert!(game.briefing_story_copy().starts_with("ROOTS ARE MOVING"));
        let chunks = LastLight::briefing_story_chunks(game.briefing_story_copy());
        assert!(chunks.iter().all(|line| line.chars().count() <= 62));
        assert!(chunks.iter().any(|line| !line.is_empty()));
    }

    #[test]
    fn briefing_grid_is_two_columns_and_keeps_pointer_rows_distinct() {
        let left = LastLight::briefing_row_rect(Vec2::ZERO, 0, 1.0);
        let right = LastLight::briefing_row_rect(Vec2::ZERO, 5, 1.0);
        let second_left = LastLight::briefing_row_rect(Vec2::ZERO, 1, 1.0);
        assert!(right.center().x > left.center().x);
        assert!(second_left.center().y < left.center().y);
        assert!(!left.intersects(right));
    }

    #[test]
    fn briefing_label_scale_preserves_short_rows_and_fits_long_upgrades() {
        assert_eq!(
            LastLight::briefing_label_scale("Z  FIELD OPTICS  60  // READY", 1.0),
            1.8
        );
        let long =
            LastLight::briefing_label_scale("C  FABRICATOR OVERCLOCK  100  // INSTALLED", 1.0);
        assert!(long < 1.8);
        assert!(long > 1.4);
    }

    #[test]
    fn mission_select_scale_keeps_small_viewports_readable() {
        assert_eq!(
            LastLight::mission_select_scale(Vec2::new(1280.0, 720.0)),
            1.0
        );
        let compact = LastLight::mission_select_scale(Vec2::new(640.0, 360.0));
        assert_eq!(compact, 0.5);
        let first = LastLight::mission_entry_rect(Vec2::ZERO, 0, compact);
        let last = LastLight::mission_entry_rect(Vec2::ZERO, 6, compact);
        assert!(first.center().y > last.center().y);
        assert!(first.size().y < 62.0);
        let footer = Aabb::from_center_size(
            Vec2::new(-320.0, -306.0 * compact),
            Vec2::new(780.0, 28.0) * compact,
        );
        assert!(!last.intersects(footer));
    }

    #[test]
    fn hud_scale_tracks_logical_viewport_without_overgrowing_cards() {
        let reference = LastLight::hud_scale_for_view(Vec2::new(1164.0, 654.0), 1.1);
        assert!((reference - (1.0 / 1.1)).abs() < 1e-5);
        assert_eq!(
            LastLight::hud_scale_for_view(Vec2::new(640.0, 360.0), 1.1),
            0.5
        );
    }

    #[test]
    fn comms_take_priority_over_the_opening_controls_hint() {
        let mut game = LastLight::new();
        game.controls_hint_remaining = 2.0;
        assert!(game.controls_hint_visible());
        game.radio_message = Some(("MARA VEY", "Stay together.", 2.0));
        assert!(!game.controls_hint_visible());
    }
}
