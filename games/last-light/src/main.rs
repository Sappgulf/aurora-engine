//! Aurora: Last Light — Reclaim the Reactor.
//! Point-and-click RTS vertical slice powered by Aurora Engine.

mod assets;
mod campaign;
mod mission_state;
mod missions;
mod save;
mod simulation;
mod units;

use std::collections::HashMap;

use assets::TextureAsset;
use aurora_engine::{
    run, Aabb, AiParams, AnimationClip, AnimationPlayer, BitmapText, Color, FogOfWar, FogState,
    FrameCtx, Game, MinimapTransform, PlacementError, PlacementRules, PointLight, PowerNodeId,
    QueueError, Renderer, SelectionBox, SimpleAggroAi, Sprite, Texture, TextureAtlas,
    TextureHandle, UnitId, UnitOrder,
};
use campaign::*;
use glam::Vec2;
use mission_state::{
    FieldBeacon, HarvestJob, HarvestPhase, ResourceKind, SalvageNode, StructureKind,
};
use missions::{DialogueTrigger, MissionDef, VictoryCondition};
use save::{CampaignStore, SaveData};
use simulation::{
    MissionOutcome, MissionSimulation, ProductionCommandError, SimulationEventKind,
    SimulationModifiers, FABRICATOR_NODE, MAP_SIZE,
};
use units::{UnitKind, CHOIR, PLAYER};
use winit::{event::MouseButton, keyboard::KeyCode};

const UNIT_ATLAS_SIZE: Vec2 = Vec2::new(1536.0, 1024.0);
const STRUCTURE_ATLAS_SIZE: Vec2 = Vec2::splat(1254.0);
const REACTION_ATLAS_SIZE: Vec2 = Vec2::new(1024.0, 1536.0);
const BEACON_COST: u32 = 50;
const COMMAND_CARD_KEYS: [KeyCode; 6] = [
    KeyCode::KeyQ,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyH,
    KeyCode::KeyT,
    KeyCode::KeyB,
];
const COMMAND_CARD_LABELS: [&str; 5] = [
    "Q  BUILD WARDEN — 90",
    "E  BUILD ENGINEER — 70",
    "F  BUILD SURVEYOR — 60",
    "H  HOLD SELECTED",
    "T  STOP SELECTED",
];

struct LastLight {
    tex_environment: TextureHandle,
    tex_units: TextureHandle,
    tex_warden_move: TextureHandle,
    tex_engineer_move: TextureHandle,
    tex_engineer_repair: TextureHandle,
    tex_surveyor_scan: TextureHandle,
    tex_needle_attack: TextureHandle,
    tex_canticle_command: TextureHandle,
    tex_bell_mine_arm: TextureHandle,
    tex_hit_reactions: TextureHandle,
    tex_down_reactions: TextureHandle,
    tex_structures: TextureHandle,
    tex_portraits: TextureHandle,
    tex_glow: TextureHandle,
    tex_ui: TextureHandle,
    unit_atlas: TextureAtlas,
    warden_move_atlas: TextureAtlas,
    engineer_move_atlas: TextureAtlas,
    engineer_repair_atlas: TextureAtlas,
    surveyor_scan_atlas: TextureAtlas,
    needle_attack_atlas: TextureAtlas,
    canticle_command_atlas: TextureAtlas,
    bell_mine_arm_atlas: TextureAtlas,
    hit_reactions_atlas: TextureAtlas,
    down_reactions_atlas: TextureAtlas,
    animation_players: HashMap<UnitId, AnimationPlayer>,
    structure_atlas: TextureAtlas,
    portrait_atlas: TextureAtlas,
    simulation: MissionSimulation,
    attack_flash: HashMap<UnitId, f32>,
    damage_flash: HashMap<UnitId, f32>,
    repair_flash: HashMap<UnitId, (UnitId, f32)>,
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
    dialogue_cursor: usize,
    radio_message: Option<(&'static str, &'static str, f32)>,
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
            tex_engineer_move: TextureHandle::default(),
            tex_engineer_repair: TextureHandle::default(),
            tex_surveyor_scan: TextureHandle::default(),
            tex_needle_attack: TextureHandle::default(),
            tex_canticle_command: TextureHandle::default(),
            tex_bell_mine_arm: TextureHandle::default(),
            tex_hit_reactions: TextureHandle::default(),
            tex_down_reactions: TextureHandle::default(),
            tex_structures: TextureHandle::default(),
            tex_portraits: TextureHandle::default(),
            tex_glow: TextureHandle::default(),
            tex_ui: TextureHandle::default(),
            unit_atlas: TextureAtlas::new(TextureHandle::default(), 3, 2, UNIT_ATLAS_SIZE),
            warden_move_atlas: TextureAtlas::new(
                TextureHandle::default(),
                6,
                1,
                Vec2::new(2172.0, 724.0),
            ),
            engineer_move_atlas: TextureAtlas::new(
                TextureHandle::default(),
                6,
                1,
                Vec2::new(1536.0, 256.0),
            ),
            engineer_repair_atlas: TextureAtlas::new(
                TextureHandle::default(),
                6,
                1,
                Vec2::new(1536.0, 256.0),
            ),
            surveyor_scan_atlas: TextureAtlas::new(
                TextureHandle::default(),
                6,
                1,
                Vec2::new(1536.0, 256.0),
            ),
            needle_attack_atlas: TextureAtlas::new(
                TextureHandle::default(),
                6,
                1,
                Vec2::new(1536.0, 256.0),
            ),
            canticle_command_atlas: TextureAtlas::new(
                TextureHandle::default(),
                6,
                1,
                Vec2::new(1536.0, 256.0),
            ),
            bell_mine_arm_atlas: TextureAtlas::new(
                TextureHandle::default(),
                6,
                1,
                Vec2::new(1536.0, 256.0),
            ),
            hit_reactions_atlas: TextureAtlas::new(
                TextureHandle::default(),
                4,
                6,
                REACTION_ATLAS_SIZE,
            ),
            down_reactions_atlas: TextureAtlas::new(
                TextureHandle::default(),
                4,
                6,
                REACTION_ATLAS_SIZE,
            ),
            animation_players: HashMap::new(),
            structure_atlas: TextureAtlas::new(
                TextureHandle::default(),
                2,
                2,
                STRUCTURE_ATLAS_SIZE,
            ),
            portrait_atlas: TextureAtlas::new(
                TextureHandle::default(),
                3,
                2,
                Vec2::new(768.0, 512.0),
            ),
            simulation,
            attack_flash: HashMap::new(),
            damage_flash: HashMap::new(),
            repair_flash: HashMap::new(),
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
            dialogue_cursor: 0,
            radio_message: None,
        }
    }

    /// Resets all mission-scoped state (world, economy, power, nav) and
    /// spawns `mission`'s roster. Campaign-wide state (`save_data`, loaded
    /// textures/atlases) is left untouched.
    fn start_mission(&mut self, mission: MissionDef) {
        let modifiers = self.simulation_modifiers();
        self.simulation = MissionSimulation::from_mission(&mission, modifiers);
        self.mission = mission;
        self.animation_players.clear();
        self.animation_players.extend(
            self.simulation
                .kinds
                .keys()
                .map(|id| (*id, AnimationPlayer::default())),
        );
        self.attack_flash.clear();
        self.damage_flash.clear();
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
        self.dialogue_cursor = 0;
        self.radio_message = None;

        self.victory_saved = false;
        self.briefing = true;
        self.paused = false;
        self.victory = false;
        self.defeat = false;
        self.enemy_think = 0.0;
        self.mission_time = 0.0;
        self.status = Some(("FABRICATOR READY — Q/E/F TO BUILD".to_owned(), 7.0));

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
            player_health: if self.save_data.campaign.has_upgrade(UPGRADE_PLATING) {
                1.2
            } else {
                1.0
            },
            player_speed: if self.specialist_module(MARA, MARA_RESCUE) == MARA_RAPID {
                1.12
            } else {
                1.0
            },
            starting_salvage: self.simulation.resources.amount(),
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
                1.0
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
                if Self::mission_entry_rect(ctx.renderer.camera.position, index)
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
        let lumen_protocol = self
            .lumen_protocol()
            .map(str::to_uppercase)
            .unwrap_or_else(|| "LOCKED — COMPLETE REACTOR".to_owned());
        let meridian_accord = self
            .meridian_accord()
            .map(str::to_uppercase)
            .unwrap_or_else(|| "LOCKED — TERMS OF SALVAGE".to_owned());
        let verdant_covenant = self
            .verdant_covenant()
            .map(str::to_uppercase)
            .unwrap_or_else(|| "LOCKED — GARDEN BELOW".to_owned());
        vec![
            (
                KeyCode::KeyZ,
                format!("Z  FIELD OPTICS — 60 LUMEN ({})", owned(UPGRADE_OPTICS)),
                Color::rgba(0.55, 0.82, 0.88, 0.98),
            ),
            (
                KeyCode::KeyX,
                format!(
                    "X  REACTIVE PLATING — 80 LUMEN ({})",
                    owned(UPGRADE_PLATING)
                ),
                Color::rgba(0.55, 0.82, 0.88, 0.98),
            ),
            (
                KeyCode::KeyC,
                format!(
                    "C  FABRICATOR OVERCLOCK — 100 LUMEN ({})",
                    owned(UPGRADE_OVERCLOCK)
                ),
                Color::rgba(0.55, 0.82, 0.88, 0.98),
            ),
            (
                KeyCode::KeyV,
                format!(
                    "V  IVO — {}",
                    self.specialist_module(IVO, IVO_RIGGER).to_uppercase()
                ),
                Color::rgba(0.82, 0.68, 0.36, 0.98),
            ),
            (
                KeyCode::KeyN,
                format!(
                    "N  SENA — {}",
                    self.specialist_module(SENA, SENA_DEEP_SCAN).to_uppercase()
                ),
                Color::rgba(0.82, 0.68, 0.36, 0.98),
            ),
            (
                KeyCode::KeyM,
                format!(
                    "M  MARA — {}",
                    self.specialist_module(MARA, MARA_RESCUE).to_uppercase()
                ),
                Color::rgba(0.7, 0.62, 0.9, 0.98),
            ),
            (
                KeyCode::KeyO,
                format!(
                    "O  OLAN — {}",
                    self.specialist_module(OLAN, OLAN_LATTICE).to_uppercase()
                ),
                Color::rgba(0.7, 0.62, 0.9, 0.98),
            ),
            (
                KeyCode::KeyL,
                format!("L  LUMEN — {lumen_protocol}"),
                Color::rgba(0.38, 0.9, 1.0, 0.98),
            ),
            (
                KeyCode::KeyP,
                format!("P  MERIDIAN — {meridian_accord}"),
                Color::rgba(0.9, 0.82, 0.72, 0.98),
            ),
            (
                KeyCode::KeyG,
                format!("G  VERDANT — {verdant_covenant}"),
                Color::rgba(0.48, 1.15, 0.5, 0.98),
            ),
        ]
    }

    fn briefing_row_rect(camera_position: Vec2, index: usize, scale: f32) -> Aabb {
        let center = camera_position + Vec2::new(-360.0, 60.0 - index as f32 * 34.0) * scale;
        Aabb::from_center_size(center, Vec2::new(600.0, 30.0) * scale)
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

    fn hud_scale(renderer: &Renderer) -> f32 {
        renderer.camera.zoom.max(f32::EPSILON).recip()
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
        const COST: u32 = 100;
        const STEP: u32 = 4;
        const MAX: u32 = 24;
        if self.simulation.supply.capacity() >= MAX {
            self.status = Some(("SUPPLY MODULE MAXED 24/24".to_owned(), 2.5));
            return;
        }
        if !self.simulation.resources.spend(COST) {
            self.status = Some(("SUPPLY MODULE REQUIRES 100 SALVAGE".to_owned(), 2.5));
            return;
        }
        let capacity = (self.simulation.supply.capacity() + STEP).min(MAX);
        self.simulation.supply.set_capacity(capacity);
        self.status = Some((format!("SUPPLY MODULE ONLINE // CAPACITY {capacity}"), 3.0));
    }

    /// Static command-card text prevents per-frame row/vector allocation.
    /// Beacon cost remains visible in placement feedback and the tooltip-like
    /// status line, while the action text itself stays allocation-free.
    fn command_card_label(&self, index: usize) -> &'static str {
        match self.selected_structure {
            Some(StructureKind::Relay(_)) => match index {
                0 => "C  GRID PULSE — 35 SALVAGE",
                1 => "ENGINEERS RESTORE OFFLINE RELAYS",
                2 => "ONLINE RELAYS GENERATE SALVAGE",
                _ => "",
            },
            Some(StructureKind::Reactor) => match index {
                0 => "C  CRAFT LUMEN CORE — 90",
                1 => "CORE: +8% LANTERN DAMAGE",
                2 => "REQUIRES FULL RELAY LATTICE",
                _ => "",
            },
            _ => match index {
                0..=4 => COMMAND_CARD_LABELS[index],
                5 if self.placing_beacon => "B  CANCEL BEACON",
                5 => "B BEACON  •  D SUPPLY",
                _ => "",
            },
        }
    }

    fn command_card_key(&self, index: usize) -> Option<KeyCode> {
        match self.selected_structure {
            Some(StructureKind::Relay(_) | StructureKind::Reactor) => {
                (index == 0).then_some(KeyCode::KeyC)
            }
            _ => COMMAND_CARD_KEYS.get(index).copied(),
        }
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

    fn command_card_row_rect(card_text: Vec2, index: usize, scale: f32) -> Aabb {
        // Production occupies the left column; squad utilities occupy the
        // right. This keeps every command visible without making a compact
        // browser viewport sacrifice the queue feedback below it.
        let column = index / 3;
        let row = index % 3;
        let center =
            card_text + Vec2::new(130.0 + column as f32 * 260.0, -38.0 - row as f32 * 30.0) * scale;
        Aabb::from_center_size(center, Vec2::new(250.0, 26.0) * scale)
    }

    fn apply_command_action(&mut self, key: KeyCode) {
        match key {
            KeyCode::KeyQ => self.queue_unit(UnitKind::Warden),
            KeyCode::KeyE => self.queue_unit(UnitKind::Engineer),
            KeyCode::KeyF => self.queue_unit(UnitKind::Surveyor),
            KeyCode::KeyH => {
                self.simulation.world.issue_hold();
                self.status = Some(("SQUAD HOLDING POSITION".to_owned(), 2.0));
            }
            KeyCode::KeyT => {
                self.simulation.world.issue_hold();
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
        bottom_right + Vec2::new(-525.0, 233.0) * scale
    }

    fn handle_command_keys(&mut self, ctx: &mut FrameCtx<'_>) {
        if ctx.input.key_pressed(KeyCode::KeyR) {
            self.focus_next_objective(ctx);
        }
        if ctx.input.key_pressed(KeyCode::KeyA) {
            self.attack_move_mode = true;
            self.patrol_mode = false;
            self.follow_mode = false;
            self.status = Some((
                "ATTACK-MOVE READY — RIGHT CLICK DESTINATION".to_owned(),
                3.0,
            ));
        }
        if ctx.input.key_pressed(KeyCode::KeyP) {
            self.patrol_mode = true;
            self.attack_move_mode = false;
            self.follow_mode = false;
            self.status = Some(("PATROL READY — RIGHT CLICK WAYPOINT".to_owned(), 3.0));
        }
        if ctx.input.key_pressed(KeyCode::KeyU) {
            self.follow_mode = true;
            self.attack_move_mode = false;
            self.patrol_mode = false;
            self.status = Some(("FOLLOW READY — RIGHT CLICK A LANTERN UNIT".to_owned(), 3.0));
        }
        if let Some(structure @ (StructureKind::Relay(_) | StructureKind::Reactor)) =
            self.selected_structure
        {
            if ctx.input.key_pressed(KeyCode::KeyC) {
                self.activate_structure_command(structure);
            }
        } else if matches!(self.selected_structure, Some(StructureKind::Fabricator))
            && ctx.input.key_pressed(KeyCode::KeyD)
        {
            self.upgrade_supply_module();
        } else {
            for key in COMMAND_CARD_KEYS {
                if ctx.input.key_pressed(key) {
                    self.apply_command_action(key);
                }
            }
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
            let card_text = Self::command_card_text_origin(ctx.renderer);
            let scale = Self::hud_scale(ctx.renderer);
            for index in 0..COMMAND_CARD_KEYS.len() {
                if Self::command_card_row_rect(card_text, index, scale).contains_point(mouse_world)
                {
                    match (self.selected_structure, self.command_card_key(index)) {
                        (
                            Some(structure @ (StructureKind::Relay(_) | StructureKind::Reactor)),
                            Some(KeyCode::KeyC),
                        ) => self.activate_structure_command(structure),
                        (_, Some(key)) => self.apply_command_action(key),
                        _ => {}
                    }
                    return;
                }
            }
            let panel = self.minimap_transform(ctx.renderer).panel;
            for slot in 1..=5 {
                if Self::control_group_chip_rect(panel, slot, scale).contains_point(mouse_world) {
                    self.control_group_action(slot, ctx.input.control_down(), ctx);
                    return;
                }
            }
        }
    }

    fn update_status_timer(&mut self, dt: f32) {
        if let Some((_, remaining)) = self.status.as_mut() {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.status = None;
            }
        }
    }

    fn process_simulation_events(&mut self, ctx: &mut FrameCtx<'_>) {
        while let Some(event) = self.simulation.pop_pending_event() {
            match event.kind {
                SimulationEventKind::RelayActivated { .. } => ctx.audio.win_note(),
                SimulationEventKind::UnitDeployed { unit_id, kind } => {
                    self.animation_players
                        .insert(UnitId(unit_id), AnimationPlayer::default());
                    self.status = Some((format!("{} DEPLOYED", kind.label()), 3.0));
                }
                SimulationEventKind::UnitSpawned { unit_id, .. } => {
                    self.animation_players
                        .insert(UnitId(unit_id), AnimationPlayer::default());
                }
                SimulationEventKind::AttackLanded { attacker, target } => {
                    self.attack_flash.insert(UnitId(attacker), 0.08);
                    self.damage_flash.insert(UnitId(target), 0.34);
                }
                SimulationEventKind::DamageApplied { target } => {
                    self.damage_flash.insert(UnitId(target), 0.34);
                }
                SimulationEventKind::UnitRepaired { engineer, target } => {
                    self.repair_flash
                        .insert(UnitId(engineer), (UnitId(target), 0.12));
                }
                SimulationEventKind::StructureRepaired { .. } => {}
                SimulationEventKind::UnitDestroyed { unit_id, .. } => {
                    let unit_id = UnitId(unit_id);
                    self.down_units.insert(unit_id, 0.0);
                    self.damage_flash.remove(&unit_id);
                }
                SimulationEventKind::BossReinforced => {
                    self.status = Some(("CANTICLE CALLS REINFORCEMENTS".to_owned(), 4.0));
                    ctx.audio.hurt();
                }
                SimulationEventKind::EnemyRaidSpawned { kind, .. } => {
                    self.status = Some((format!("CHOIR RAID // {} INBOUND", kind.label()), 4.0));
                    ctx.audio.hurt();
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
                (distance <= unit.radius * 1.8).then_some((unit.id, distance))
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
            .position(|node| node.remaining > 0 && node.position.distance(point) <= 95.0)
    }

    fn assign_harvest_order(&mut self, node: usize) -> usize {
        let Some(position) = self.salvage_nodes.get(node).map(|node| node.position) else {
            return 0;
        };
        let surveyors: Vec<UnitId> = self
            .simulation
            .world
            .selection()
            .ids()
            .iter()
            .copied()
            .filter(|id| self.simulation.kinds.get(id) == Some(&UnitKind::Surveyor))
            .collect();
        for surveyor in &surveyors {
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
        surveyors.len()
    }

    fn friendly_unit_at(&self, point: Vec2) -> bool {
        self.simulation.world.units().iter().any(|unit| {
            unit.faction == PLAYER
                && unit.alive()
                && unit.position.distance(point) <= unit.radius * 1.35
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
                (distance <= unit.radius * 1.6).then_some((unit.id, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(id, _)| id)
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

    fn structure_status_line(&self, structure: StructureKind) -> String {
        let condition = self
            .simulation
            .structure(structure)
            .map(|state| {
                format!(
                    "  HP {:.0}/{:.0}  BUILD {:02}%",
                    state.health,
                    state.max_health,
                    state.build_progress * 100.0
                )
            })
            .unwrap_or_default();
        match structure {
            StructureKind::Relay(index) => match self.simulation.relays.get(index) {
                Some(relay) if relay.active => format!("RELAY — ONLINE{condition}"),
                Some(relay) => format!(
                    "RELAY — CHARGING {:.0}%  (ENGINEER NEARBY TO RESTORE){condition}",
                    (relay.progress / 3.0 * 100.0).clamp(0.0, 100.0)
                ),
                None => "RELAY".to_owned(),
            },
            StructureKind::Fabricator => format!(
                "LANTERN FABRICATOR — {}  QUEUE {}/5  {}{}",
                if self.simulation.power.is_powered(FABRICATOR_NODE) {
                    "POWERED"
                } else {
                    "OFFLINE"
                },
                self.simulation.production.items().len(),
                if self.simulation.rally_point.is_some() {
                    "RALLY SET"
                } else {
                    "RIGHT CLICK TO SET RALLY"
                },
                condition
            ),
            StructureKind::Reactor => format!(
                "AUXILIARY REACTOR — {}{}",
                if self
                    .simulation
                    .tech
                    .is_unlocked(crate::simulation::TECH_RELAY_NETWORK)
                {
                    "LATTICE ONLINE"
                } else {
                    "AWAITING FULL POWER LATTICE"
                },
                condition
            ),
        }
    }

    /// Resolves the next player-facing objective from the same mission state
    /// that determines victory. The result drives the HUD, minimap, world
    /// beacon, and camera-focus key so those surfaces cannot disagree.
    fn next_objective(&self) -> Option<(Vec2, String)> {
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
        self.status = Some((format!("OBJECTIVE FOCUS — {label}"), 2.5));
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
                        self.simulation.world.clear_selection();
                    } else if !ctx.input.shift_down()
                        && !self.simulation.world.selection().ids().is_empty()
                        && !self.friendly_unit_at(mouse_world)
                    {
                        // A selected squad can also use the intuitive
                        // select-then-left-click terrain command. This keeps
                        // browser play viable when a secondary click is
                        // intercepted by the host platform.
                        self.selected_structure = None;
                        self.issue_move_order(mouse_world);
                        ctx.audio.collect();
                    } else {
                        self.selected_structure = None;
                        self.simulation.world.select_point(
                            mouse_world,
                            PLAYER,
                            ctx.input.shift_down(),
                        );
                    }
                } else {
                    self.selected_structure = None;
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
                    self.status = Some(("SELECT A SURVEYOR TO HARVEST".to_owned(), 2.5));
                }
            } else if let Some(enemy) = self.closest_enemy_at(mouse_world) {
                self.simulation.world.issue_attack(enemy);
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

    fn update_enemy_ai(&mut self, dt: f32) {
        self.enemy_think -= dt;
        if self.enemy_think > 0.0 {
            return;
        }
        self.enemy_think = 0.65;
        // Let the player read the battlefield and issue an opening order before
        // the Choir begins reacting. Afterward, patrols only engage contacts
        // inside their local sensor envelope instead of map-wide rushing.
        if self.mission_time < 8.0 {
            return;
        }
        self.enemy_ai.think(
            &mut self.simulation.world,
            CHOIR,
            PLAYER,
            self.mission_time,
            &AiParams::default(),
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
        const EXTRACT_PER_SECOND: f32 = 18.0;
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
                        remove.push(id);
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
                    node.harvest_buffer += dt.max(0.0) * EXTRACT_PER_SECOND;
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
                        } else {
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

    fn update_radio_dialogue(&mut self, dt: f32) {
        if let Some((_, _, remaining)) = self.radio_message.as_mut() {
            *remaining -= dt.max(0.0);
            if *remaining > 0.0 {
                return;
            }
            self.radio_message = None;
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
        };
        if ready {
            self.radio_message = Some((line.speaker, line.text, 6.0));
            self.dialogue_cursor += 1;
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

    fn mission_entry_rect(camera_position: Vec2, index: usize) -> Aabb {
        let center = camera_position + Vec2::new(0.0, 140.0 - index as f32 * 74.0);
        Aabb::from_center_size(center, Vec2::new(780.0, 62.0))
    }

    fn draw_mission_select(&self, ctx: &mut FrameCtx<'_>) {
        self.draw_full_screen_backdrop(ctx, Color::rgba(0.01, 0.02, 0.045, 0.82));
        let center = ctx.renderer.camera.position;
        self.draw_text_shadowed(
            ctx.renderer,
            "AURORA: LAST LIGHT",
            center + Vec2::new(-320.0, 330.0),
            7.5,
            Color::rgb(0.32, 1.55, 1.35),
            11.0,
        );
        self.draw_text_shadowed(
            ctx.renderer,
            "SELECT MISSION",
            center + Vec2::new(-320.0, 210.0),
            3.4,
            Color::rgba(0.7, 0.85, 0.9, 0.95),
            11.0,
        );
        for (index, mission) in missions::all().iter().enumerate() {
            let unlocked = self.save_data.campaign.unlocked_mission >= mission.required_tier;
            let rect = Self::mission_entry_rect(center, index);
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
                rect.min + Vec2::new(24.0, 22.0),
                3.4,
                color,
                11.0,
            );
        }
        self.draw_text_shadowed(
            ctx.renderer,
            "CLICK A MISSION   OR  UP/DOWN + SPACE/ENTER",
            center + Vec2::new(-320.0, -270.0),
            2.4,
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
            engineer_move,
            engineer_repair,
            surveyor_scan,
            needle_attack,
            canticle_command,
            bell_mine_arm,
            hit_reactions,
            down_reactions,
            structures,
            portraits,
            glow,
            ui,
        ) = {
            let gpu = renderer.gpu();
            (
                assets::load_texture(&gpu, TextureAsset::ReactorSector),
                assets::load_texture(&gpu, TextureAsset::Units),
                assets::load_texture(&gpu, TextureAsset::WardenMove),
                assets::load_texture(&gpu, TextureAsset::EngineerMove),
                assets::load_texture(&gpu, TextureAsset::EngineerRepair),
                assets::load_texture(&gpu, TextureAsset::SurveyorScan),
                assets::load_texture(&gpu, TextureAsset::NeedleAttack),
                assets::load_texture(&gpu, TextureAsset::CanticleCommand),
                assets::load_texture(&gpu, TextureAsset::BellMineArm),
                assets::load_texture(&gpu, TextureAsset::HitReactions),
                assets::load_texture(&gpu, TextureAsset::DownReactions),
                assets::load_texture(&gpu, TextureAsset::Structures),
                assets::load_texture(&gpu, TextureAsset::CommandPortraits),
                Texture::soft_circle(&gpu, 64, Color::WHITE),
                Texture::solid(&gpu, Color::WHITE),
            )
        };
        self.tex_environment = renderer.add_texture(environment);
        self.tex_units = renderer.add_texture(units);
        self.tex_warden_move = renderer.add_texture(warden_move);
        self.tex_engineer_move = renderer.add_texture(engineer_move);
        self.tex_engineer_repair = renderer.add_texture(engineer_repair);
        self.tex_surveyor_scan = renderer.add_texture(surveyor_scan);
        self.tex_needle_attack = renderer.add_texture(needle_attack);
        self.tex_canticle_command = renderer.add_texture(canticle_command);
        self.tex_bell_mine_arm = renderer.add_texture(bell_mine_arm);
        self.tex_hit_reactions = renderer.add_texture(hit_reactions);
        self.tex_down_reactions = renderer.add_texture(down_reactions);
        self.tex_structures = renderer.add_texture(structures);
        self.tex_portraits = renderer.add_texture(portraits);
        self.tex_glow = renderer.add_texture(glow);
        self.tex_ui = renderer.add_texture(ui);
        self.unit_atlas = TextureAtlas::new(self.tex_units, 3, 2, UNIT_ATLAS_SIZE);
        self.warden_move_atlas =
            TextureAtlas::new(self.tex_warden_move, 6, 1, Vec2::new(2172.0, 724.0));
        self.engineer_move_atlas =
            TextureAtlas::new(self.tex_engineer_move, 6, 1, Vec2::new(1536.0, 256.0));
        self.engineer_repair_atlas =
            TextureAtlas::new(self.tex_engineer_repair, 6, 1, Vec2::new(1536.0, 256.0));
        self.surveyor_scan_atlas =
            TextureAtlas::new(self.tex_surveyor_scan, 6, 1, Vec2::new(1536.0, 256.0));
        self.needle_attack_atlas =
            TextureAtlas::new(self.tex_needle_attack, 6, 1, Vec2::new(1536.0, 256.0));
        self.canticle_command_atlas =
            TextureAtlas::new(self.tex_canticle_command, 6, 1, Vec2::new(1536.0, 256.0));
        self.bell_mine_arm_atlas =
            TextureAtlas::new(self.tex_bell_mine_arm, 6, 1, Vec2::new(1536.0, 256.0));
        self.hit_reactions_atlas =
            TextureAtlas::new(self.tex_hit_reactions, 4, 6, REACTION_ATLAS_SIZE);
        self.down_reactions_atlas =
            TextureAtlas::new(self.tex_down_reactions, 4, 6, REACTION_ATLAS_SIZE);
        self.structure_atlas = TextureAtlas::new(self.tex_structures, 2, 2, STRUCTURE_ATLAS_SIZE);
        self.portrait_atlas = TextureAtlas::new(self.tex_portraits, 3, 2, Vec2::new(768.0, 512.0));
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
        if self.paused || self.victory || self.defeat {
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
            let kind = self.simulation.kinds.get(&unit.id).copied();
            let engaged = unit.alive() && self.unit_engaged(unit.id);
            let Some(player) = self.animation_players.get_mut(&unit.id) else {
                continue;
            };
            let reaction_base = kind.map_or(0, |unit_kind| unit_kind.atlas_frame() * 4);
            let clip = if !unit.alive() {
                Some(AnimationClip::once(
                    "down",
                    (reaction_base..reaction_base + 4).collect::<Vec<_>>(),
                    6.0,
                ))
            } else if self.damage_flash.contains_key(&unit.id) {
                Some(AnimationClip::once(
                    "hit",
                    (reaction_base..reaction_base + 4).collect::<Vec<_>>(),
                    14.0,
                ))
            } else {
                match kind {
                    Some(UnitKind::Warden) if engaged => {
                        Some(AnimationClip::looping("attack", [0, 1, 2, 3, 4, 5], 14.0))
                    }
                    Some(UnitKind::Warden) if unit.velocity.length_squared() > 1.0 => {
                        Some(AnimationClip::looping("move", [0, 1, 2, 3, 4, 5], 10.0))
                    }
                    Some(UnitKind::Engineer) if unit.velocity.length_squared() > 1.0 => {
                        Some(AnimationClip::looping("move", [0, 1, 2, 3, 4, 5], 9.0))
                    }
                    Some(UnitKind::Engineer) if self.repair_flash.contains_key(&unit.id) => {
                        Some(AnimationClip::looping("repair", [0, 1, 2, 3, 4, 5], 12.0))
                    }
                    Some(UnitKind::Surveyor) => {
                        Some(AnimationClip::looping("scan", [0, 1, 2, 3, 4, 5], 7.0))
                    }
                    Some(UnitKind::Needle) if engaged => {
                        Some(AnimationClip::looping("attack", [0, 1, 2, 3, 4, 5], 11.0))
                    }
                    Some(UnitKind::Canticle) if engaged => {
                        Some(AnimationClip::looping("command", [0, 1, 2, 3, 4, 5], 7.5))
                    }
                    Some(UnitKind::BellMine) if engaged => {
                        Some(AnimationClip::looping("arm", [0, 1, 2, 3, 4, 5], 8.5))
                    }
                    _ => None,
                }
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
                self.save_data.campaign.record_decision(LUMEN_AWAKENED);
                let _ = self
                    .save_store
                    .save(&save::envelope(self.save_data.clone()));
                self.status = Some(("LUMEN CONSOLE AWAKENED".to_owned(), 4.0));
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
        let t = ctx.time.elapsed;
        if self.mission_select {
            self.draw_mission_select(ctx);
            return;
        }
        ctx.renderer.draw_sprite(
            self.tex_environment,
            Sprite::new(Vec2::ZERO, MAP_SIZE).with_z(-10.0),
        );
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

        for node in &self.salvage_nodes {
            let charge = node.remaining as f32 / 240.0;
            if charge <= 0.0 {
                continue;
            }
            let pulse = 0.82 + (t * 3.2 + node.position.x * 0.01).sin() * 0.12;
            let node_color = match node.kind {
                ResourceKind::Salvage => Color::rgba(0.08, 1.4, 1.35, 0.18 * pulse),
                ResourceKind::Flux => Color::rgba(0.72, 0.2, 1.55, 0.22 * pulse),
            };
            ctx.renderer.draw_sprite(
                self.tex_glow,
                Sprite::new(node.position, Vec2::splat(150.0 * charge.max(0.45)))
                    .with_color(node_color)
                    .with_z(-0.25),
            );
            for rotation in [0.0_f32, 2.094, 4.188] {
                let offset = Vec2::new(rotation.cos(), rotation.sin()) * 32.0;
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(
                        node.position + offset,
                        Vec2::new(34.0, 62.0 * charge.max(0.32)),
                    )
                    .with_rotation(rotation - std::f32::consts::FRAC_PI_2)
                    .with_color(match node.kind {
                        ResourceKind::Salvage => Color::rgba(0.15, 1.35, 1.4, 0.92),
                        ResourceKind::Flux => Color::rgba(0.76, 0.28, 1.6, 0.95),
                    })
                    .with_z(-0.05),
                );
            }
        }

        for relay in &self.simulation.relays {
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
            let engaged = self.unit_engaged(unit.id);
            let animated = if self.damage_flash.contains_key(&unit.id) {
                Some((self.tex_hit_reactions, &self.hit_reactions_atlas))
            } else {
                match kind {
                    UnitKind::Warden if engaged => {
                        Some((self.tex_warden_move, &self.warden_move_atlas))
                    }
                    UnitKind::Warden if unit.velocity.length_squared() > 1.0 => {
                        Some((self.tex_warden_move, &self.warden_move_atlas))
                    }
                    UnitKind::Engineer if unit.velocity.length_squared() > 1.0 => {
                        Some((self.tex_engineer_move, &self.engineer_move_atlas))
                    }
                    UnitKind::Engineer if self.repair_flash.contains_key(&unit.id) => {
                        Some((self.tex_engineer_repair, &self.engineer_repair_atlas))
                    }
                    UnitKind::Surveyor => Some((self.tex_surveyor_scan, &self.surveyor_scan_atlas)),
                    UnitKind::Needle if engaged => {
                        Some((self.tex_needle_attack, &self.needle_attack_atlas))
                    }
                    UnitKind::Canticle if engaged => {
                        Some((self.tex_canticle_command, &self.canticle_command_atlas))
                    }
                    UnitKind::BellMine if engaged => {
                        Some((self.tex_bell_mine_arm, &self.bell_mine_arm_atlas))
                    }
                    _ => None,
                }
            };
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
            if unit.velocity.length_squared() > 1.0 {
                sprite.rotation =
                    unit.velocity.y.atan2(unit.velocity.x) - std::f32::consts::FRAC_PI_2;
            }
            sprite.z = 1.0;
            ctx.renderer.draw_sprite(texture, sprite);

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

        if !self.briefing && !self.paused && !self.victory && !self.defeat {
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
        let top_left = ctx
            .renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(0.0, 1.0))
            + Vec2::new(30.0, -34.0) * hud_scale;
        // Keep combat telemetry legible over the active world without turning
        // the entire top edge into permanent chrome.
        ctx.renderer.draw_sprite(
            self.tex_ui,
            Sprite::new(
                top_left + Vec2::new(280.0, -58.0) * hud_scale,
                Vec2::new(590.0, 146.0) * hud_scale,
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
            VictoryCondition::RestoreRelaysAndDefeatBoss { .. } => format!(
                "{}  RELAYS {active_relays}/{}",
                self.mission.title,
                self.simulation.relays.len()
            ),
            VictoryCondition::EscortToExtraction { point, .. } => {
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
        };
        self.draw_text(
            ctx.renderer,
            &objective_line,
            top_left,
            3.35 * hud_scale,
            Color::rgb(0.73, 1.15, 1.08),
            8.0,
        );
        let control_hint = if self.simulation.world.selection().ids().is_empty() {
            "DRAG SELECT  •  TERRAIN MOVE  •  A ATTACK-MOVE  •  P PATROL  •  U FOLLOW"
        } else if self
            .simulation
            .world
            .selection()
            .ids()
            .iter()
            .any(|id| self.simulation.kinds.get(id) == Some(&UnitKind::Surveyor))
        {
            "RIGHT CLICK SALVAGE TO HARVEST  •  A ATTACK-MOVE  •  P PATROL  •  U FOLLOW"
        } else {
            "CLICK TERRAIN MOVE  •  SHIFT+RIGHT QUEUE  •  A ATTACK-MOVE  •  P PATROL  •  U FOLLOW"
        };
        self.draw_text(
            ctx.renderer,
            control_hint,
            top_left + Vec2::new(0.0, -25.0) * hud_scale,
            1.9 * hud_scale,
            Color::rgba(0.58, 0.7, 0.78, 0.86),
            8.0,
        );
        let income = active_relays * self.relay_income() as usize;
        let cargo: u32 = self.harvest_jobs.values().map(|job| job.cargo).sum();
        self.draw_text(
            ctx.renderer,
            &format!(
                "SALVAGE {}  FLUX {}  CARGO {cargo}  LUMEN CORES {}",
                self.simulation.resources.amount(),
                self.simulation.flux,
                self.lumen_cores,
            ),
            top_left + Vec2::new(0.0, -50.0) * hud_scale,
            2.8 * hud_scale,
            Color::rgb(0.96, 0.72, 0.28),
            8.0,
        );
        self.draw_text(
            ctx.renderer,
            &format!(
                "INCOME +{income}/S  POWER {}/{}  SUPPLY {}/{}",
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
        if let Some(selected) = self.simulation.world.selection().ids().first() {
            let count = self.simulation.world.selection().ids().len();
            let kind = self.simulation.kinds[selected];
            if self
                .simulation
                .world
                .unit(*selected)
                .is_some_and(|unit| unit.faction == PLAYER)
            {
                let unit_card = ctx
                    .renderer
                    .camera
                    .world_from_viewport_fraction(Vec2::new(0.0, 0.0))
                    + Vec2::new(300.0, 18.0) * hud_scale;
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(
                        unit_card + Vec2::new(210.0, 58.0) * hud_scale,
                        Vec2::new(420.0, 116.0) * hud_scale,
                    )
                    .with_color(Color::rgba(0.01, 0.025, 0.05, 0.88))
                    .with_z(7.7),
                );
                let portrait = self.portrait_atlas.sprite(
                    unit_card + Vec2::new(58.0, 58.0) * hud_scale,
                    Vec2::new(108.0, 108.0) * hud_scale,
                    Self::unit_portrait_frame(kind),
                );
                ctx.renderer
                    .draw_sprite(self.tex_portraits, portrait.with_z(8.1));
                if let Some(unit) = self.simulation.world.unit(*selected) {
                    self.draw_text(
                        ctx.renderer,
                        &format!(
                            "HP {:03}/{:03}  //  {}",
                            unit.health.ceil() as u32,
                            unit.max_health.ceil() as u32,
                            Self::order_label(unit.order)
                        ),
                        unit_card + Vec2::new(122.0, 65.0) * hud_scale,
                        1.9 * hud_scale,
                        Color::rgb(0.72, 0.92, 0.9),
                        8.1,
                    );
                    self.draw_text(
                        ctx.renderer,
                        match kind {
                            UnitKind::Warden => "FRONTLINE // GUARD & SUPPRESS",
                            UnitKind::Engineer => "SUPPORT // AUTO-REPAIR & RESTORE",
                            UnitKind::Surveyor => "RECON // SCAN, MARK & HARVEST",
                            _ => "CONTACT // HOSTILE",
                        },
                        unit_card + Vec2::new(122.0, 39.0) * hud_scale,
                        1.55 * hud_scale,
                        Color::rgba(0.55, 0.75, 0.78, 0.9),
                        8.1,
                    );
                }
            }
            self.draw_text(
                ctx.renderer,
                &format!("{}  //  SQUAD {count}", kind.label()),
                top_left + Vec2::new(0.0, -101.0) * hud_scale,
                2.7 * hud_scale,
                Color::rgb(0.96, 0.72, 0.28),
                8.0,
            );
        } else if let Some(structure) = self.selected_structure {
            self.draw_text(
                ctx.renderer,
                &self.structure_status_line(structure),
                top_left + Vec2::new(0.0, -101.0) * hud_scale,
                2.6 * hud_scale,
                Color::rgb(0.4, 0.95, 1.0),
                8.0,
            );
        }
        if let Some(message) = self.engineer_relay_status() {
            self.draw_text(
                ctx.renderer,
                &message,
                top_left + Vec2::new(0.0, -127.0) * hud_scale,
                2.2 * hud_scale,
                Color::rgb(0.3, 1.35, 1.18),
                8.0,
            );
        } else if let Some((message, _)) = &self.status {
            self.draw_text(
                ctx.renderer,
                message,
                top_left + Vec2::new(0.0, -127.0) * hud_scale,
                2.5 * hud_scale,
                Color::rgb(0.65, 1.15, 1.05),
                8.0,
            );
        }
        if let Some((_, objective)) = self.next_objective() {
            self.draw_text(
                ctx.renderer,
                &format!("NEXT // {objective}"),
                top_left + Vec2::new(0.0, -153.0) * hud_scale,
                1.75 * hud_scale,
                Color::rgba(1.08, 0.72, 0.28, 0.92),
                8.0,
            );
        }

        if let Some((speaker, line, _)) = self.radio_message {
            let top_right = ctx
                .renderer
                .camera
                .world_from_viewport_fraction(Vec2::new(1.0, 1.0));
            let origin = top_right + Vec2::new(-540.0, -42.0) * hud_scale;
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    origin + Vec2::new(250.0, -34.0) * hud_scale,
                    Vec2::new(520.0, 88.0) * hud_scale,
                )
                .with_color(Color::rgba(0.025, 0.055, 0.085, 0.9))
                .with_z(8.6),
            );
            let portrait = self.portrait_atlas.sprite(
                origin + Vec2::new(40.0, -34.0) * hud_scale,
                Vec2::new(76.0, 76.0) * hud_scale,
                Self::speaker_portrait_frame(speaker),
            );
            ctx.renderer
                .draw_sprite(self.tex_portraits, portrait.with_z(8.75));
            self.draw_text(
                ctx.renderer,
                &format!("COMMS // {speaker}"),
                origin + Vec2::new(88.0, 0.0) * hud_scale,
                1.9 * hud_scale,
                Color::rgb(0.3, 1.4, 1.2),
                8.8,
            );
            self.draw_text(
                ctx.renderer,
                line,
                origin + Vec2::new(88.0, -30.0) * hud_scale,
                1.25 * hud_scale,
                Color::rgb(0.88, 0.92, 0.92),
                8.8,
            );
        }

        if !self.briefing && !self.paused && !self.victory && !self.defeat {
            let card_text = Self::command_card_text_origin(ctx.renderer);
            let card_center = card_text + Vec2::new(240.0, -104.5) * hud_scale;
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(card_center, Vec2::new(530.0, 300.0) * hud_scale)
                    .with_color(Color::rgba(0.01, 0.025, 0.05, 0.88))
                    .with_z(7.5),
            );
            let card_title = match self.selected_structure {
                Some(StructureKind::Relay(_)) => "POWER RELAY",
                Some(StructureKind::Reactor) => "AUXILIARY REACTOR",
                _ => "LANTERN FABRICATOR",
            };
            self.draw_text(
                ctx.renderer,
                card_title,
                card_text,
                2.8 * hud_scale,
                Color::rgb(0.3, 1.4, 1.2),
                8.0,
            );
            let mouse_world = ctx
                .renderer
                .camera
                .screen_to_world(ctx.input.mouse_position);
            for (index, _) in COMMAND_CARD_KEYS.iter().enumerate() {
                let rect = Self::command_card_row_rect(card_text, index, hud_scale);
                let hovered = rect.contains_point(mouse_world);
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(rect.center(), rect.size())
                        .with_color(if hovered {
                            Color::rgba(0.16, 0.55, 0.6, 0.35)
                        } else {
                            Color::rgba(0.04, 0.08, 0.12, 0.3)
                        })
                        .with_z(7.6),
                );
                self.draw_text(
                    ctx.renderer,
                    self.command_card_label(index),
                    rect.min + Vec2::new(8.0, 8.0) * hud_scale,
                    1.9 * hud_scale,
                    Color::rgb(0.88, 0.92, 0.92),
                    8.0,
                );
            }
            self.draw_text(
                ctx.renderer,
                "CMD/CTRL+1-5 ASSIGN   1-5 OR CLICK RECALL",
                card_text + Vec2::new(0.0, -142.0) * hud_scale,
                1.6 * hud_scale,
                Color::rgba(0.55, 0.7, 0.78, 0.9),
                8.0,
            );
            let front_progress = matches!(
                self.selected_structure,
                None | Some(StructureKind::Fabricator)
            )
            .then(|| {
                self.simulation
                    .production
                    .items()
                    .front()
                    .map(|item| item.progress())
            })
            .flatten();
            let queue_label = match self.selected_structure {
                Some(structure @ (StructureKind::Relay(_) | StructureKind::Reactor)) => {
                    self.structure_status_line(structure)
                }
                _ => self
                    .simulation
                    .production
                    .items()
                    .front()
                    .map(|item| {
                        let label = UnitKind::from_product(item.product)
                            .map(UnitKind::label)
                            .unwrap_or("UNKNOWN");
                        format!(
                            "BUILDING {label}  {:02}%  QUEUE {}",
                            (item.progress() * 100.0) as u32,
                            self.simulation.production.items().len()
                        )
                    })
                    .unwrap_or_else(|| "QUEUE READY".to_owned()),
            };
            self.draw_text(
                ctx.renderer,
                &queue_label,
                card_text + Vec2::new(0.0, -166.0) * hud_scale,
                2.0 * hud_scale,
                Color::rgb(1.15, 0.7, 0.25),
                8.0,
            );
            if let Some(progress) = front_progress {
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
                "RESTART THE GAME TO RETRY".to_owned(),
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
                    &format!("LUMEN AVAILABLE: {}", self.save_data.campaign.currency),
                    center + Vec2::new(-360.0, 110.0) * overlay_scale,
                    2.2 * overlay_scale,
                    Color::rgba(0.75, 0.9, 0.95, 0.95),
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
                        1.8 * overlay_scale,
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
    }

    #[test]
    fn starting_reclaim_the_reactor_populates_relays_and_roster() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        assert_eq!(game.simulation.relays.len(), 3);
        assert_eq!(game.salvage_nodes.len(), 5);
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

        game.simulation.world.unit_mut(surveyor).unwrap().position = game.fabricator_position;
        assert_eq!(game.update_harvesting(0.0), 24);
        assert_eq!(game.simulation.resources.amount(), salvage_before + 24);
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
            Some("ESCORT SENA TO THE ARRAY".to_owned())
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
}
