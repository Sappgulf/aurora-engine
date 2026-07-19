//! Aurora: Last Light — Reclaim the Reactor.
//! Point-and-click RTS vertical slice powered by Aurora Engine.

mod assets;
mod campaign;
mod mission_state;
mod missions;
mod save;
mod units;

use std::collections::{HashMap, VecDeque};

use assets::TextureAsset;
use aurora_engine::{
    mark_obstacles, run, Aabb, AiParams, AnimationClip, AnimationPlayer, BitmapText, Color,
    FactionId, FogOfWar, FogState, FrameCtx, Game, MinimapTransform, NavGrid, PlacementError,
    PlacementRules, PointLight, PowerGrid, PowerNode, PowerNodeId, ProductionQueue, QueueError,
    Renderer, ResourceBank, RtsWorld, SelectionBox, SimpleAggroAi, Sprite, Texture, TextureAtlas,
    TextureHandle, UnitId, UnitOrder,
};
use campaign::*;
use glam::Vec2;
use mission_state::{FieldBeacon, Relay, StructureKind};
use missions::{MissionDef, VictoryCondition};
use save::{CampaignStore, SaveData};
use units::{UnitKind, CHOIR, PLAYER};
use winit::{event::MouseButton, keyboard::KeyCode};

const MAP_SIZE: Vec2 = Vec2::new(2600.0, 1460.0);
const NAV_CELL_SIZE: f32 = 40.0;
const UNIT_ATLAS_SIZE: Vec2 = Vec2::new(1536.0, 1024.0);
const STRUCTURE_ATLAS_SIZE: Vec2 = Vec2::splat(1254.0);
const REACTION_ATLAS_SIZE: Vec2 = Vec2::new(1024.0, 1536.0);
const FABRICATOR_NODE: PowerNodeId = PowerNodeId(0);
const BEACON_COST: u32 = 50;

struct LastLight {
    tex_environment: TextureHandle,
    tex_units: TextureHandle,
    tex_warden_move: TextureHandle,
    tex_engineer_move: TextureHandle,
    tex_surveyor_scan: TextureHandle,
    tex_needle_attack: TextureHandle,
    tex_canticle_command: TextureHandle,
    tex_bell_mine_arm: TextureHandle,
    tex_hit_reactions: TextureHandle,
    tex_down_reactions: TextureHandle,
    tex_structures: TextureHandle,
    tex_glow: TextureHandle,
    tex_ui: TextureHandle,
    unit_atlas: TextureAtlas,
    warden_move_atlas: TextureAtlas,
    engineer_move_atlas: TextureAtlas,
    surveyor_scan_atlas: TextureAtlas,
    needle_attack_atlas: TextureAtlas,
    canticle_command_atlas: TextureAtlas,
    bell_mine_arm_atlas: TextureAtlas,
    hit_reactions_atlas: TextureAtlas,
    down_reactions_atlas: TextureAtlas,
    animation_players: HashMap<UnitId, AnimationPlayer>,
    structure_atlas: TextureAtlas,
    world: RtsWorld,
    kinds: HashMap<UnitId, UnitKind>,
    attack_flash: HashMap<UnitId, f32>,
    damage_flash: HashMap<UnitId, f32>,
    down_units: HashMap<UnitId, f32>,
    fog: FogOfWar,
    drag: Option<SelectionBox>,
    order_marker: Option<(Vec2, f32)>,
    relays: Vec<Relay>,
    reactor_position: Option<Vec2>,
    fabricator_position: Vec2,
    field_beacons: Vec<FieldBeacon>,
    placing_beacon: bool,
    resources: ResourceBank,
    resource_tick: f32,
    production: ProductionQueue,
    power: PowerGrid,
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
    escort_unit: Option<UnitId>,
    enemy_ai: SimpleAggroAi,
    nav: NavGrid,
    selected_structure: Option<StructureKind>,
    /// Remaining waypoints for units routed around an obstacle by
    /// `handle_pointer`'s move command. Advanced in `advance_player_paths`.
    player_paths: HashMap<UnitId, VecDeque<Vec2>>,
    canticle_reinforced: bool,
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

        Self {
            tex_environment: TextureHandle::default(),
            tex_units: TextureHandle::default(),
            tex_warden_move: TextureHandle::default(),
            tex_engineer_move: TextureHandle::default(),
            tex_surveyor_scan: TextureHandle::default(),
            tex_needle_attack: TextureHandle::default(),
            tex_canticle_command: TextureHandle::default(),
            tex_bell_mine_arm: TextureHandle::default(),
            tex_hit_reactions: TextureHandle::default(),
            tex_down_reactions: TextureHandle::default(),
            tex_structures: TextureHandle::default(),
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
            world: RtsWorld::default(),
            kinds: HashMap::new(),
            attack_flash: HashMap::new(),
            damage_flash: HashMap::new(),
            down_units: HashMap::new(),
            fog: FogOfWar::new(26, 15, -MAP_SIZE * 0.5, 100.0),
            drag: None,
            order_marker: None,
            relays: Vec::new(),
            reactor_position: None,
            fabricator_position: Vec2::ZERO,
            field_beacons: Vec::new(),
            placing_beacon: false,
            resources: ResourceBank::new(starting_salvage),
            resource_tick: 0.0,
            production: ProductionQueue::new(5),
            power: PowerGrid::default(),
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
            mission: missions::reclaim_the_reactor(),
            escort_unit: None,
            enemy_ai: SimpleAggroAi::new(),
            nav: NavGrid::new(1, 1, Vec2::ZERO, NAV_CELL_SIZE),
            selected_structure: None,
            player_paths: HashMap::new(),
            canticle_reinforced: false,
        }
    }

    /// Resets all mission-scoped state (world, economy, power, nav) and
    /// spawns `mission`'s roster. Campaign-wide state (`save_data`, loaded
    /// textures/atlases) is left untouched.
    fn start_mission(&mut self, mission: MissionDef) {
        self.mission = mission;
        self.world = RtsWorld::default();
        self.kinds.clear();
        self.animation_players.clear();
        self.attack_flash.clear();
        self.damage_flash.clear();
        self.down_units.clear();
        self.fog = FogOfWar::new(26, 15, -MAP_SIZE * 0.5, 100.0);
        self.drag = None;
        self.order_marker = None;
        self.relays = self
            .mission
            .relays
            .iter()
            .map(|&position| Relay {
                position,
                progress: 0.0,
                active: false,
            })
            .collect();
        self.reactor_position = self.mission.reactor_position;
        self.fabricator_position = self.mission.fabricator_position;
        self.field_beacons.clear();
        self.placing_beacon = false;
        self.resource_tick = 0.0;
        self.production = ProductionQueue::new(5);
        self.escort_unit = None;
        self.enemy_ai = SimpleAggroAi::new();
        self.selected_structure = None;
        self.player_paths.clear();
        self.canticle_reinforced = false;

        let mut power = PowerGrid::default();
        power.add_node(PowerNode {
            id: FABRICATOR_NODE,
            supply: 1,
            demand: 1,
            online: true,
        });
        for index in 0..self.mission.relays.len() {
            let relay = PowerNodeId(index as u16 + 1);
            power.add_node(PowerNode {
                id: relay,
                supply: 1,
                demand: 0,
                online: false,
            });
            power.link(FABRICATOR_NODE, relay);
        }
        self.power = power;

        let mut nav = NavGrid::new(
            (MAP_SIZE.x / NAV_CELL_SIZE).ceil() as usize,
            (MAP_SIZE.y / NAV_CELL_SIZE).ceil() as usize,
            -MAP_SIZE * 0.5,
            NAV_CELL_SIZE,
        );
        mark_obstacles(&mut nav, &self.mission.obstacles);
        self.nav = nav;

        self.victory_saved = false;
        self.briefing = true;
        self.paused = false;
        self.victory = false;
        self.defeat = false;
        self.enemy_think = 0.0;
        self.mission_time = 0.0;
        self.status = Some(("FABRICATOR READY — Q/E/F TO BUILD".to_owned(), 7.0));

        self.populate_mission();
    }

    fn populate_mission(&mut self) {
        let player_spawns = self.mission.player_spawns.clone();
        for spawn in player_spawns {
            let id = self.spawn(
                spawn.kind,
                PLAYER,
                spawn.position,
                spawn.health,
                spawn.speed,
            );
            if spawn.escort {
                self.escort_unit = Some(id);
            }
        }
        let enemy_spawns = self.mission.enemy_spawns.clone();
        for spawn in enemy_spawns {
            self.spawn(spawn.kind, CHOIR, spawn.position, spawn.health, spawn.speed);
        }
    }

    fn spawn(
        &mut self,
        kind: UnitKind,
        faction: FactionId,
        position: Vec2,
        health: f32,
        speed: f32,
    ) -> UnitId {
        let id = self.world.spawn(faction, position);
        let health = if faction == PLAYER && self.save_data.campaign.has_upgrade(UPGRADE_PLATING) {
            health * 1.2
        } else {
            health
        };
        let speed = if faction == PLAYER && self.specialist_module(MARA, MARA_RESCUE) == MARA_RAPID
        {
            speed * 1.12
        } else {
            speed
        };
        if let Some(unit) = self.world.unit_mut(id) {
            unit.health = health;
            unit.max_health = health;
            unit.speed = speed;
            unit.radius = kind.scale() * 0.27;
        }
        self.kinds.insert(id, kind);
        self.animation_players
            .insert(id, AnimationPlayer::default());
        id
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

    fn briefing_row_rect(camera_position: Vec2, index: usize) -> Aabb {
        let center = camera_position + Vec2::new(-360.0, 60.0 - index as f32 * 34.0);
        Aabb::from_center_size(center, Vec2::new(600.0, 30.0))
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
            for index in 0..row_count {
                if Self::briefing_row_rect(ctx.renderer.camera.position, index)
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
            self.relays
                .iter()
                .filter(|relay| relay.active)
                .map(|relay| relay.position),
        );
        power_sources.extend(self.field_beacons.iter().map(|beacon| beacon.position));
        let mut obstructions = vec![(self.fabricator_position, 105.0)];
        if let Some(reactor_position) = self.reactor_position {
            obstructions.push((reactor_position, 135.0));
        }
        obstructions.extend(self.relays.iter().map(|relay| (relay.position, 85.0)));
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
        let bottom_left = renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(0.0, 0.0));
        MinimapTransform {
            world: Aabb::from_center_size(Vec2::ZERO, MAP_SIZE),
            panel: Aabb::from_center_size(
                bottom_left + Vec2::new(150.0, 92.0),
                Vec2::new(260.0, 138.0),
            ),
        }
    }

    fn friendly_count(&self) -> usize {
        self.world
            .units()
            .iter()
            .filter(|unit| unit.faction == PLAYER && unit.alive())
            .count()
    }

    fn queue_unit(&mut self, kind: UnitKind) {
        if self.friendly_count() + self.production.items().len() >= 12 {
            self.status = Some(("UNIT CAP 12".to_owned(), 2.5));
            return;
        }
        if !self.power.is_powered(FABRICATOR_NODE) {
            self.status = Some(("FABRICATOR OFFLINE".to_owned(), 2.5));
            return;
        }
        let Some(mut recipe) = kind.recipe() else {
            return;
        };
        if self.save_data.campaign.has_upgrade(UPGRADE_OVERCLOCK) {
            recipe.build_millis = (recipe.build_millis as f32 * 0.75) as u32;
        }
        match self.production.enqueue(recipe, &mut self.resources) {
            Ok(()) => {
                self.status = Some((format!("{} ADDED TO QUEUE", kind.label()), 2.5));
            }
            Err(QueueError::InsufficientResources) => {
                self.status = Some(("INSUFFICIENT SALVAGE".to_owned(), 2.5));
            }
            Err(QueueError::Full) => {
                self.status = Some(("PRODUCTION QUEUE FULL".to_owned(), 2.5));
            }
        }
    }

    /// One row per command-card action. A single source of truth for both
    /// the on-screen card text and its click hit-boxes, mirroring
    /// `briefing_rows`/`briefing_row_rect`.
    fn command_card_rows(&self) -> Vec<(KeyCode, String)> {
        vec![
            (KeyCode::KeyQ, "Q  BUILD WARDEN — 90".to_owned()),
            (KeyCode::KeyE, "E  BUILD ENGINEER — 70".to_owned()),
            (KeyCode::KeyF, "F  BUILD SURVEYOR — 60".to_owned()),
            (KeyCode::KeyH, "H  HOLD SELECTED".to_owned()),
            (KeyCode::KeyT, "T  STOP SELECTED".to_owned()),
            (
                KeyCode::KeyB,
                format!(
                    "B  {} BEACON — {}",
                    if self.placing_beacon {
                        "CANCEL"
                    } else {
                        "PLACE"
                    },
                    self.beacon_cost()
                ),
            ),
        ]
    }

    fn command_card_row_rect(card_text: Vec2, index: usize) -> Aabb {
        // Production occupies the left column; squad utilities occupy the
        // right. This keeps every command visible without making a compact
        // browser viewport sacrifice the queue feedback below it.
        let column = index / 3;
        let row = index % 3;
        let center =
            card_text + Vec2::new(130.0 + column as f32 * 260.0, -38.0 - row as f32 * 30.0);
        Aabb::from_center_size(center, Vec2::new(250.0, 26.0))
    }

    fn apply_command_action(&mut self, key: KeyCode) {
        match key {
            KeyCode::KeyQ => self.queue_unit(UnitKind::Warden),
            KeyCode::KeyE => self.queue_unit(UnitKind::Engineer),
            KeyCode::KeyF => self.queue_unit(UnitKind::Surveyor),
            KeyCode::KeyH => {
                self.world.issue_hold();
                self.status = Some(("SQUAD HOLDING POSITION".to_owned(), 2.0));
            }
            KeyCode::KeyT => {
                self.world.issue_hold();
                self.player_paths.clear();
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
            self.world.assign_control_group(slot);
            self.status = Some((format!("CONTROL GROUP {slot} ASSIGNED"), 2.0));
        } else if self.world.recall_control_group(slot, PLAYER) {
            let (sum, count) = self
                .world
                .selection()
                .ids()
                .iter()
                .filter_map(|id| self.world.unit(*id))
                .fold((Vec2::ZERO, 0_u32), |(sum, count), unit| {
                    (sum + unit.position, count + 1)
                });
            if count > 0 {
                ctx.renderer.camera.position = sum / count as f32;
            }
            self.status = Some((format!("CONTROL GROUP {slot}"), 1.5));
        }
    }

    fn control_group_chip_rect(panel: Aabb, slot: usize) -> Aabb {
        const SPACING: f32 = 46.0;
        let start_x = panel.center().x - SPACING * 2.0;
        let x = start_x + (slot - 1) as f32 * SPACING;
        let y = panel.max.y + 26.0;
        Aabb::from_center_size(Vec2::new(x, y), Vec2::splat(38.0))
    }

    fn pause_icon_rect(renderer: &Renderer) -> Aabb {
        let top_right = renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(1.0, 1.0));
        Aabb::from_center_size(top_right + Vec2::new(-44.0, -44.0), Vec2::splat(48.0))
    }

    /// World-space anchor for the command card's text/rows. The single
    /// source of truth for both rendering (`on_update`) and click
    /// hit-testing (`handle_command_keys`) so they can't drift apart.
    fn command_card_text_origin(renderer: &Renderer) -> Vec2 {
        let bottom_right = renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(1.0, 0.0));
        bottom_right + Vec2::new(-525.0, 233.0)
    }

    fn handle_command_keys(&mut self, ctx: &mut FrameCtx<'_>) {
        let rows = self.command_card_rows();
        for (key, _) in &rows {
            if ctx.input.key_pressed(*key) {
                self.apply_command_action(*key);
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
            for (index, (key, _)) in rows.iter().enumerate() {
                if Self::command_card_row_rect(card_text, index).contains_point(mouse_world) {
                    self.apply_command_action(*key);
                    return;
                }
            }
            let panel = self.minimap_transform(ctx.renderer).panel;
            for slot in 1..=5 {
                if Self::control_group_chip_rect(panel, slot).contains_point(mouse_world) {
                    self.control_group_action(slot, ctx.input.control_down(), ctx);
                    return;
                }
            }
        }
    }

    fn update_economy(&mut self, dt: f32) {
        let active_relays = self.relays.iter().filter(|relay| relay.active).count() as f32;
        self.resource_tick += dt.max(0.0) * active_relays * self.relay_income() as f32;
        let income = self.resource_tick.floor() as u32;
        if income > 0 {
            self.resources.credit(income);
            self.resource_tick -= income as f32;
        }

        if self.power.is_powered(FABRICATOR_NODE) {
            for product in self.production.update(dt) {
                let Some(kind) = UnitKind::from_product(product) else {
                    continue;
                };
                let offset = Vec2::new(80.0, (self.friendly_count() % 3) as f32 * 70.0 - 70.0);
                let (health, speed) = match kind {
                    UnitKind::Warden => (155.0, 175.0),
                    UnitKind::Engineer => (115.0, 150.0),
                    UnitKind::Surveyor => (90.0, 215.0),
                    UnitKind::Needle | UnitKind::Canticle | UnitKind::BellMine => continue,
                };
                self.spawn(
                    kind,
                    PLAYER,
                    self.fabricator_position + offset,
                    health,
                    speed,
                );
                self.status = Some((format!("{} DEPLOYED", kind.label()), 3.0));
            }
        }

        if let Some((_, remaining)) = self.status.as_mut() {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.status = None;
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

    /// Recomputes `self.victory`/`self.defeat` from live world state and the
    /// active mission's [`VictoryCondition`]. Pure aside from those two
    /// fields (no `FrameCtx` needed), so it can run in tests without a GPU.
    fn evaluate_mission_state(&mut self) {
        let friendlies_alive = self
            .world
            .units()
            .iter()
            .any(|unit| unit.faction == PLAYER && unit.alive());
        let escort_failed = matches!(
            self.mission.victory,
            VictoryCondition::EscortToExtraction { .. }
        ) && !self
            .escort_unit
            .and_then(|id| self.world.unit(id))
            .is_some_and(|unit| unit.alive());
        self.defeat = !friendlies_alive || escort_failed;
        self.victory = match self.mission.victory {
            VictoryCondition::RestoreRelaysAndDefeatBoss { boss_kind } => {
                let boss_alive = self.world.units().iter().any(|unit| {
                    unit.faction == CHOIR
                        && unit.alive()
                        && self.kinds.get(&unit.id) == Some(&boss_kind)
                });
                self.relays.iter().all(|relay| relay.active) && !boss_alive
            }
            VictoryCondition::EscortToExtraction { point, radius } => self
                .escort_unit
                .and_then(|id| self.world.unit(id))
                .is_some_and(|unit| unit.alive() && unit.position.distance(point) <= radius),
        };
    }

    fn selected_engineer_near(&self, position: Vec2) -> bool {
        self.world.selection().ids().iter().any(|id| {
            self.kinds.get(id) == Some(&UnitKind::Engineer)
                && self
                    .world
                    .unit(*id)
                    .is_some_and(|unit| unit.alive() && unit.position.distance(position) < 110.0)
        })
    }

    /// A high-priority, contextual explanation for the Engineer's relay job.
    /// It deliberately derives from selection + distance, the same conditions
    /// that advance relay progress, so the HUD cannot promise an interaction
    /// the simulation will not perform.
    fn engineer_relay_status(&self) -> Option<String> {
        self.relays.iter().enumerate().find_map(|(index, relay)| {
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
        self.world
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
        self.relays
            .iter()
            .position(|relay| relay.position.distance(point) <= StructureKind::RELAY_RADIUS)
            .map(StructureKind::Relay)
    }

    fn structure_position(&self, structure: StructureKind) -> Option<Vec2> {
        match structure {
            StructureKind::Relay(index) => self.relays.get(index).map(|relay| relay.position),
            StructureKind::Fabricator => Some(self.fabricator_position),
            StructureKind::Reactor => self.reactor_position,
        }
    }

    fn structure_status_line(&self, structure: StructureKind) -> String {
        match structure {
            StructureKind::Relay(index) => match self.relays.get(index) {
                Some(relay) if relay.active => "RELAY — ONLINE".to_owned(),
                Some(relay) => format!(
                    "RELAY — CHARGING {:.0}%  (ENGINEER NEARBY TO RESTORE)",
                    (relay.progress / 3.0 * 100.0).clamp(0.0, 100.0)
                ),
                None => "RELAY".to_owned(),
            },
            StructureKind::Fabricator => format!(
                "LANTERN FABRICATOR — {}  QUEUE {}/5",
                if self.power.is_powered(FABRICATOR_NODE) {
                    "POWERED"
                } else {
                    "OFFLINE"
                },
                self.production.items().len()
            ),
            StructureKind::Reactor => "AUXILIARY REACTOR — AWAITING FULL POWER LATTICE".to_owned(),
        }
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
                    Ok(()) if self.resources.spend(beacon_cost) => {
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
                    } else {
                        self.selected_structure = None;
                        self.world
                            .select_point(mouse_world, PLAYER, ctx.input.shift_down());
                    }
                } else {
                    self.selected_structure = None;
                    self.world
                        .select_bounds(drag.bounds(), PLAYER, ctx.input.shift_down());
                }
            }
        }
        if ctx.input.mouse_pressed(MouseButton::Right) && !self.world.selection().ids().is_empty() {
            let selected_ids = self.world.selection().ids().to_vec();
            if let Some(enemy) = self.closest_enemy_at(mouse_world) {
                self.world.issue_attack(enemy);
                for id in &selected_ids {
                    self.player_paths.remove(id);
                }
            } else {
                self.world.issue_move(mouse_world, 74.0);
                self.route_around_obstacles(&selected_ids);
            }
            self.order_marker = Some((mouse_world, 0.65));
            ctx.audio.collect();
        }
    }

    /// After `RtsWorld::issue_move` sets each unit's formation destination,
    /// replace any destination whose straight line crosses a mission
    /// obstacle with a route through `self.nav`, queuing the remaining
    /// waypoints for `advance_player_paths` to walk through. No-op on
    /// missions with no obstacles (`self.nav` has nothing blocked).
    fn route_around_obstacles(&mut self, selected_ids: &[UnitId]) {
        for &id in selected_ids {
            let Some(unit) = self.world.unit(id) else {
                continue;
            };
            let UnitOrder::Move(destination) = unit.order else {
                continue;
            };
            if self.nav.segment_blocked(unit.position, destination) {
                let mut path: VecDeque<Vec2> =
                    self.nav.find_path(unit.position, destination).into();
                if let Some(first) = path.pop_front() {
                    if let Some(unit) = self.world.unit_mut(id) {
                        unit.order = UnitOrder::Move(first);
                    }
                    self.player_paths.insert(id, path);
                    continue;
                }
            }
            self.player_paths.remove(&id);
        }
    }

    /// Advances any unit whose queued route (see `route_around_obstacles`)
    /// has more waypoints once it arrives (`RtsWorld::update` sets a
    /// completed `Move` order to `Idle`).
    fn advance_player_paths(&mut self) {
        if self.player_paths.is_empty() {
            return;
        }
        let ids: Vec<UnitId> = self.player_paths.keys().copied().collect();
        for id in ids {
            let Some(unit) = self.world.unit(id) else {
                self.player_paths.remove(&id);
                continue;
            };
            if !matches!(unit.order, UnitOrder::Idle) {
                continue;
            }
            let done = match self.player_paths.get_mut(&id) {
                Some(queue) => {
                    if let Some(next) = queue.pop_front() {
                        if let Some(unit) = self.world.unit_mut(id) {
                            unit.order = UnitOrder::Move(next);
                        }
                    }
                    queue.is_empty()
                }
                None => true,
            };
            if done {
                self.player_paths.remove(&id);
            }
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
            &mut self.world,
            CHOIR,
            PLAYER,
            self.mission_time,
            &AiParams::default(),
            Some(&self.nav),
        );
    }

    fn unit_engaged(&self, id: UnitId) -> bool {
        let Some(unit) = self.world.unit(id) else {
            return false;
        };
        let UnitOrder::Attack(target) = unit.order else {
            return false;
        };
        let Some(range) = self.kinds.get(&id).map(|kind| kind.combat().range) else {
            return false;
        };
        self.world.unit(target).is_some_and(|target| {
            target.alive() && unit.position.distance(target.position) <= range
        })
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

    /// Gives the Canticle boss a one-time "call reinforcements" beat: the
    /// first time it drops to half health, it spawns two extra Needles at
    /// its position. Purely additive to the shared combat/AI systems (no
    /// unique per-boss code paths elsewhere), so it stays cheap to check
    /// every tick and self-disarms via `canticle_reinforced`.
    /// Returns `true` the one time it triggers, so the caller can play a cue
    /// without this needing a `FrameCtx` (keeps it directly unit-testable).
    fn update_boss_phase(&mut self) -> bool {
        if self.canticle_reinforced {
            return false;
        }
        let low_health_canticle = self.world.units().iter().find_map(|unit| {
            let is_canticle = unit.faction == CHOIR
                && unit.alive()
                && self.kinds.get(&unit.id) == Some(&UnitKind::Canticle);
            let low_health = unit.health / unit.max_health.max(1.0) <= 0.5;
            (is_canticle && low_health).then_some(unit.position)
        });
        let Some(position) = low_health_canticle else {
            return false;
        };
        self.canticle_reinforced = true;
        for offset in [Vec2::new(-90.0, 40.0), Vec2::new(90.0, -40.0)] {
            self.spawn(UnitKind::Needle, CHOIR, position + offset, 90.0, 125.0);
        }
        self.status = Some(("CANTICLE CALLS REINFORCEMENTS".to_owned(), 4.0));
        true
    }

    fn update_combat(&mut self, dt: f32) {
        let snapshot: HashMap<UnitId, (Vec2, bool)> = self
            .world
            .units()
            .iter()
            .map(|unit| (unit.id, (unit.position, unit.alive())))
            .collect();
        let mut damage = Vec::new();
        for unit in self.world.units() {
            let UnitOrder::Attack(target) = unit.order else {
                continue;
            };
            let Some((target_position, true)) = snapshot.get(&target).copied() else {
                continue;
            };
            let Some(profile) = self.kinds.get(&unit.id).map(|kind| kind.combat()) else {
                continue;
            };
            if unit.position.distance(target_position) < profile.range {
                let mut dps = profile.damage_per_second;
                if unit.faction == PLAYER
                    && self.specialist_module(SENA, SENA_DEEP_SCAN) == SENA_GHOST_MARK
                {
                    dps *= 1.15;
                }
                if unit.faction == PLAYER
                    && self.specialist_module(OLAN, OLAN_LATTICE) == OLAN_DECODER
                {
                    dps *= 1.1;
                }
                damage.push((target, dps * dt));
                self.attack_flash.insert(unit.id, 0.08);
            }
        }
        if self.verdant_covenant() == Some(VERDANT_BRIAR) {
            for unit in self
                .world
                .units()
                .iter()
                .filter(|unit| unit.faction == CHOIR && unit.alive())
            {
                if self
                    .field_beacons
                    .iter()
                    .any(|beacon| beacon.position.distance(unit.position) <= 220.0)
                {
                    damage.push((unit.id, 8.0 * dt.max(0.0)));
                }
            }
        }
        let bastion_accord = self.meridian_accord() == Some(MERIDIAN_BASTION);
        for (target, amount) in damage {
            if let Some(unit) = self.world.unit_mut(target) {
                let was_alive = unit.alive();
                let amount = if unit.faction == PLAYER && bastion_accord {
                    amount * 0.82
                } else {
                    amount
                };
                unit.health = (unit.health - amount).max(0.0);
                self.damage_flash.insert(target, 0.34);
                if was_alive && !unit.alive() {
                    self.down_units.insert(target, 0.0);
                    self.damage_flash.remove(&target);
                }
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
        for age in self.down_units.values_mut() {
            *age += dt.max(0.0);
        }
    }

    fn update_fog(&mut self) {
        self.fog.begin_frame();
        for unit in self
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == PLAYER && unit.alive())
        {
            let radius = if self.kinds.get(&unit.id) == Some(&UnitKind::Surveyor) {
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
    /// These are the same `Aabb`s that block `self.nav`, so what's drawn
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
            surveyor_scan,
            needle_attack,
            canticle_command,
            bell_mine_arm,
            hit_reactions,
            down_reactions,
            structures,
            glow,
            ui,
        ) = {
            let gpu = renderer.gpu();
            (
                assets::load_texture(&gpu, TextureAsset::ReactorSector),
                assets::load_texture(&gpu, TextureAsset::Units),
                assets::load_texture(&gpu, TextureAsset::WardenMove),
                assets::load_texture(&gpu, TextureAsset::EngineerMove),
                assets::load_texture(&gpu, TextureAsset::SurveyorScan),
                assets::load_texture(&gpu, TextureAsset::NeedleAttack),
                assets::load_texture(&gpu, TextureAsset::CanticleCommand),
                assets::load_texture(&gpu, TextureAsset::BellMineArm),
                assets::load_texture(&gpu, TextureAsset::HitReactions),
                assets::load_texture(&gpu, TextureAsset::DownReactions),
                assets::load_texture(&gpu, TextureAsset::Structures),
                Texture::soft_circle(&gpu, 64, Color::WHITE),
                Texture::solid(&gpu, Color::WHITE),
            )
        };
        self.tex_environment = renderer.add_texture(environment);
        self.tex_units = renderer.add_texture(units);
        self.tex_warden_move = renderer.add_texture(warden_move);
        self.tex_engineer_move = renderer.add_texture(engineer_move);
        self.tex_surveyor_scan = renderer.add_texture(surveyor_scan);
        self.tex_needle_attack = renderer.add_texture(needle_attack);
        self.tex_canticle_command = renderer.add_texture(canticle_command);
        self.tex_bell_mine_arm = renderer.add_texture(bell_mine_arm);
        self.tex_hit_reactions = renderer.add_texture(hit_reactions);
        self.tex_down_reactions = renderer.add_texture(down_reactions);
        self.tex_structures = renderer.add_texture(structures);
        self.tex_glow = renderer.add_texture(glow);
        self.tex_ui = renderer.add_texture(ui);
        self.unit_atlas = TextureAtlas::new(self.tex_units, 3, 2, UNIT_ATLAS_SIZE);
        self.warden_move_atlas =
            TextureAtlas::new(self.tex_warden_move, 6, 1, Vec2::new(2172.0, 724.0));
        self.engineer_move_atlas =
            TextureAtlas::new(self.tex_engineer_move, 6, 1, Vec2::new(1536.0, 256.0));
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
        self.world.update(dt);
        self.advance_player_paths();
        for unit in self.world.units() {
            let kind = self.kinds.get(&unit.id).copied();
            let engaged = unit.alive()
                && matches!(
                    kind,
                    Some(UnitKind::Needle | UnitKind::Canticle | UnitKind::BellMine)
                )
                && self.unit_engaged(unit.id);
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
                    Some(UnitKind::Warden) if unit.velocity.length_squared() > 1.0 => {
                        Some(AnimationClip::looping("move", [0, 1, 2, 3, 4, 5], 10.0))
                    }
                    Some(UnitKind::Engineer) if unit.velocity.length_squared() > 1.0 => {
                        Some(AnimationClip::looping("move", [0, 1, 2, 3, 4, 5], 9.0))
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
        self.update_combat(dt);
        if self.update_boss_phase() {
            ctx.audio.hurt();
        }
        self.update_specialist_doctrines(dt);
        self.update_fog();
        self.update_economy(dt);
        self.mission_time += dt;
        if let Some((_, time)) = self.order_marker.as_mut() {
            *time -= dt;
            if *time <= 0.0 {
                self.order_marker = None;
            }
        }

        for index in 0..self.relays.len() {
            if self.relays[index].active {
                continue;
            }
            let position = self.relays[index].position;
            if self.selected_engineer_near(position) {
                let rate = if self.specialist_module(IVO, IVO_RIGGER) == IVO_RIGGER {
                    1.5
                } else {
                    1.0
                };
                self.relays[index].progress += dt * rate;
                if self.relays[index].progress >= 3.0 {
                    self.relays[index].progress = 3.0;
                    self.relays[index].active = true;
                    self.power.set_online(PowerNodeId(index as u16 + 1), true);
                    ctx.audio.win_note();
                }
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

        for (index, relay) in self.relays.iter().enumerate() {
            if !relay.active || !self.power.is_powered(PowerNodeId(index as u16 + 1)) {
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

        for relay in &self.relays {
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
                && self.resources.amount() >= self.beacon_cost();
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

        for unit in self.world.units() {
            if unit.faction == CHOIR {
                let fog_state = self.fog.state_at(unit.position);
                if (unit.alive() && fog_state != FogState::Visible)
                    || (!unit.alive() && fog_state == FogState::Hidden)
                {
                    continue;
                }
            }
            let kind = self.kinds[&unit.id];
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
            let selected = self.world.selection().contains(unit.id);
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
            let engaged = matches!(
                kind,
                UnitKind::Needle | UnitKind::Canticle | UnitKind::BellMine
            ) && self.unit_engaged(unit.id);
            let animated = if self.damage_flash.contains_key(&unit.id) {
                Some((self.tex_hit_reactions, &self.hit_reactions_atlas))
            } else {
                match kind {
                    UnitKind::Warden if unit.velocity.length_squared() > 1.0 => {
                        Some((self.tex_warden_move, &self.warden_move_atlas))
                    }
                    UnitKind::Engineer if unit.velocity.length_squared() > 1.0 => {
                        Some((self.tex_engineer_move, &self.engineer_move_atlas))
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

        if !self.briefing && !self.paused && !self.victory && !self.defeat {
            let minimap = self.minimap_transform(ctx.renderer);
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    minimap.panel.center(),
                    minimap.panel.size() + Vec2::splat(12.0),
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
            for relay in &self.relays {
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(minimap.world_to_panel(relay.position), Vec2::splat(7.0))
                        .with_color(if relay.active {
                            Color::rgb(0.2, 1.5, 1.2)
                        } else {
                            Color::rgb(0.45, 0.5, 0.55)
                        })
                        .with_z(8.0),
                );
            }
            for unit in self.world.units().iter().filter(|unit| unit.alive()) {
                if unit.faction == CHOIR && self.fog.state_at(unit.position) != FogState::Visible {
                    continue;
                }
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(minimap.world_to_panel(unit.position), Vec2::splat(5.0))
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
                (Vec2::new(center.x, map_min.y), Vec2::new(size.x, 2.0)),
                (Vec2::new(center.x, map_max.y), Vec2::new(size.x, 2.0)),
                (Vec2::new(map_min.x, center.y), Vec2::new(2.0, size.y)),
                (Vec2::new(map_max.x, center.y), Vec2::new(2.0, size.y)),
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
                let rect = Self::control_group_chip_rect(minimap.panel, slot);
                let count = self.world.control_group(slot).len();
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
                    rect.min + Vec2::new(4.0, 8.0),
                    1.6,
                    Color::rgb(0.85, 0.95, 0.95),
                    8.4,
                );
            }
        }

        {
            let rect = Self::pause_icon_rect(ctx.renderer);
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(rect.center(), rect.size())
                    .with_color(Color::rgba(0.04, 0.08, 0.12, 0.75))
                    .with_z(9.0),
            );
            let bar_size = Vec2::new(6.0, 22.0);
            for offset in [Vec2::new(-7.0, 0.0), Vec2::new(7.0, 0.0)] {
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(rect.center() + offset, bar_size)
                        .with_color(Color::rgb(0.85, 0.9, 0.95))
                        .with_z(9.1),
                );
            }
        }

        let top_left = ctx
            .renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(0.0, 1.0))
            + Vec2::new(30.0, -34.0);
        // Keep combat telemetry legible over the active world without turning
        // the entire top edge into permanent chrome.
        ctx.renderer.draw_sprite(
            self.tex_ui,
            Sprite::new(top_left + Vec2::new(280.0, -58.0), Vec2::new(590.0, 146.0))
                .with_color(Color::rgba(0.01, 0.025, 0.05, 0.68))
                .with_z(7.5),
        );
        let active_relays = self.relays.iter().filter(|relay| relay.active).count();
        let objective_line = match self.mission.victory {
            VictoryCondition::RestoreRelaysAndDefeatBoss { .. } => format!(
                "{}  RELAYS {active_relays}/{}",
                self.mission.title,
                self.relays.len()
            ),
            VictoryCondition::EscortToExtraction { point, .. } => {
                let escort_status = self
                    .escort_unit
                    .and_then(|id| self.world.unit(id))
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
            3.35,
            Color::rgb(0.73, 1.15, 1.08),
            8.0,
        );
        let control_hint = if self.world.selection().ids().is_empty() {
            "DRAG SELECT  •  RIGHT CLICK MOVE / ATTACK  •  B BEACON"
        } else {
            "RIGHT CLICK COMMAND  •  H HOLD  •  T STOP  •  B BEACON"
        };
        self.draw_text(
            ctx.renderer,
            control_hint,
            top_left + Vec2::new(0.0, -25.0),
            1.9,
            Color::rgba(0.58, 0.7, 0.78, 0.86),
            8.0,
        );
        let income = active_relays * self.relay_income() as usize;
        self.draw_text(
            ctx.renderer,
            &format!(
                "SALVAGE {}  +{income}/S  POWER {}/{}  UNITS {}/12",
                self.resources.amount(),
                active_relays + 1,
                self.relays.len() + 1,
                self.friendly_count()
            ),
            top_left + Vec2::new(0.0, -50.0),
            2.8,
            Color::rgb(0.96, 0.72, 0.28),
            8.0,
        );
        if let Some(selected) = self.world.selection().ids().first() {
            let count = self.world.selection().ids().len();
            self.draw_text(
                ctx.renderer,
                &format!("{}  //  SQUAD {count}", self.kinds[selected].label()),
                top_left + Vec2::new(0.0, -75.0),
                2.7,
                Color::rgb(0.96, 0.72, 0.28),
                8.0,
            );
        } else if let Some(structure) = self.selected_structure {
            self.draw_text(
                ctx.renderer,
                &self.structure_status_line(structure),
                top_left + Vec2::new(0.0, -75.0),
                2.6,
                Color::rgb(0.4, 0.95, 1.0),
                8.0,
            );
        }
        if let Some(message) = self.engineer_relay_status() {
            self.draw_text(
                ctx.renderer,
                &message,
                top_left + Vec2::new(0.0, -100.0),
                2.2,
                Color::rgb(0.3, 1.35, 1.18),
                8.0,
            );
        } else if let Some((message, _)) = &self.status {
            self.draw_text(
                ctx.renderer,
                message,
                top_left + Vec2::new(0.0, -100.0),
                2.5,
                Color::rgb(0.65, 1.15, 1.05),
                8.0,
            );
        }

        if !self.briefing && !self.paused && !self.victory && !self.defeat {
            let card_text = Self::command_card_text_origin(ctx.renderer);
            let card_center = card_text + Vec2::new(240.0, -104.5);
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(card_center, Vec2::new(530.0, 300.0))
                    .with_color(Color::rgba(0.01, 0.025, 0.05, 0.88))
                    .with_z(7.5),
            );
            self.draw_text(
                ctx.renderer,
                "LANTERN FABRICATOR",
                card_text,
                2.8,
                Color::rgb(0.3, 1.4, 1.2),
                8.0,
            );
            let mouse_world = ctx
                .renderer
                .camera
                .screen_to_world(ctx.input.mouse_position);
            for (index, (_, label)) in self.command_card_rows().iter().enumerate() {
                let rect = Self::command_card_row_rect(card_text, index);
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
                    label,
                    rect.min + Vec2::new(8.0, 8.0),
                    1.9,
                    Color::rgb(0.88, 0.92, 0.92),
                    8.0,
                );
            }
            self.draw_text(
                ctx.renderer,
                "CMD/CTRL+1-5 ASSIGN   1-5 OR CLICK RECALL",
                card_text + Vec2::new(0.0, -142.0),
                1.6,
                Color::rgba(0.55, 0.7, 0.78, 0.9),
                8.0,
            );
            let front_progress = self.production.items().front().map(|item| item.progress());
            let queue_label = self
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
                        self.production.items().len()
                    )
                })
                .unwrap_or_else(|| "QUEUE READY".to_owned());
            self.draw_text(
                ctx.renderer,
                &queue_label,
                card_text + Vec2::new(0.0, -166.0),
                2.0,
                Color::rgb(1.15, 0.7, 0.25),
                8.0,
            );
            if let Some(progress) = front_progress {
                let bar_origin = card_text + Vec2::new(0.0, -188.0);
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(bar_origin + Vec2::new(150.0, 0.0), Vec2::new(300.0, 8.0))
                        .with_color(Color::rgba(0.1, 0.1, 0.12, 0.9))
                        .with_z(8.0),
                );
                ctx.renderer.draw_sprite(
                    self.tex_ui,
                    Sprite::new(
                        bar_origin + Vec2::new(300.0 * progress * 0.5, 0.0),
                        Vec2::new(300.0 * progress, 8.0),
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
            self.draw_full_screen_backdrop(ctx, Color::rgba(0.01, 0.02, 0.045, 0.8));
            let center = ctx.renderer.camera.position;
            self.draw_text_shadowed(
                ctx.renderer,
                title,
                center + Vec2::new(-view.x * 0.42, view.y * 0.36),
                6.5,
                title_color,
                11.0,
            );
            self.draw_text_shadowed(
                ctx.renderer,
                story,
                center + Vec2::new(-view.x * 0.42, view.y * 0.36 - 55.0),
                2.1,
                Color::rgb(0.8, 0.88, 0.9),
                11.0,
            );
            self.draw_text_shadowed(
                ctx.renderer,
                &prompt,
                center + Vec2::new(-view.x * 0.42, -view.y * 0.4),
                3.2,
                Color::rgb(1.25, 0.78, 0.28),
                11.0,
            );
            if self.briefing {
                self.draw_text_shadowed(
                    ctx.renderer,
                    &format!("LUMEN AVAILABLE: {}", self.save_data.campaign.currency),
                    center + Vec2::new(-360.0, 110.0),
                    2.2,
                    Color::rgba(0.75, 0.9, 0.95, 0.95),
                    11.0,
                );
                let mouse_world = ctx
                    .renderer
                    .camera
                    .screen_to_world(ctx.input.mouse_position);
                for (index, (_, label, color)) in self.briefing_rows().iter().enumerate() {
                    let rect = Self::briefing_row_rect(center, index);
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
                        rect.min + Vec2::new(14.0, 10.0),
                        1.8,
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
        assert_eq!(game.relays.len(), 3);
        assert!(game.reactor_position.is_some());
        assert_eq!(game.friendly_count(), 3);
        assert_eq!(
            game.world
                .units()
                .iter()
                .filter(|unit| unit.faction == CHOIR)
                .count(),
            6
        );
    }

    #[test]
    fn selected_engineer_reports_relay_restoration_job() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let engineer = game
            .kinds
            .iter()
            .find(|(_, kind)| **kind == UnitKind::Engineer)
            .map(|(id, _)| *id)
            .expect("mission includes an engineer");
        let relay_position = game.relays[0].position;
        game.world.unit_mut(engineer).unwrap().position = relay_position;
        game.world.select_point(relay_position, PLAYER, false);

        assert_eq!(
            game.engineer_relay_status().as_deref(),
            Some("ENGINEER LINK // RELAY 1 — RESTORING 00%")
        );
    }

    #[test]
    fn reclaim_the_reactor_victory_requires_relays_and_boss_dead() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        game.evaluate_mission_state();
        assert!(!game.victory);

        for relay in &mut game.relays {
            relay.active = true;
        }
        game.evaluate_mission_state();
        assert!(!game.victory, "boss is still alive");

        let canticle_ids: Vec<_> = game
            .kinds
            .iter()
            .filter(|(_, kind)| **kind == UnitKind::Canticle)
            .map(|(id, _)| *id)
            .collect();
        for id in canticle_ids {
            if let Some(unit) = game.world.unit_mut(id) {
                unit.health = 0.0;
            }
        }
        game.evaluate_mission_state();
        assert!(game.victory);
    }

    #[test]
    fn voice_in_conduit_twelve_tracks_escort_survival_and_extraction() {
        let mut game = LastLight::new();
        game.start_mission(missions::voice_in_conduit_twelve());
        let escort = game.escort_unit.expect("mission defines an escort spawn");
        game.evaluate_mission_state();
        assert!(!game.victory);
        assert!(!game.defeat);

        let VictoryCondition::EscortToExtraction { point, .. } = game.mission.victory else {
            panic!("expected an escort victory condition");
        };
        game.world.unit_mut(escort).unwrap().position = point;
        game.evaluate_mission_state();
        assert!(game.victory);

        game.world.unit_mut(escort).unwrap().health = 0.0;
        game.evaluate_mission_state();
        assert!(game.defeat);
    }

    #[test]
    fn enemy_ai_routes_around_conduit_obstacles() {
        let mut game = LastLight::new();
        game.start_mission(missions::voice_in_conduit_twelve());
        // The mission's obstacles should mark at least one nav cell blocked.
        let blocked = game.nav.is_blocked_at(Vec2::new(-500.0, 480.0));
        assert!(blocked, "corridor wall should block its own center cell");
    }

    #[test]
    fn canticle_calls_reinforcements_once_at_half_health() {
        let mut game = LastLight::new();
        game.start_mission(missions::reclaim_the_reactor());
        let canticle_id = game
            .kinds
            .iter()
            .find(|(_, kind)| **kind == UnitKind::Canticle)
            .map(|(id, _)| *id)
            .expect("mission spawns a Canticle");
        let choir_before = game
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == CHOIR)
            .count();

        // Still above half health: no trigger yet.
        assert!(!game.update_boss_phase());
        assert_eq!(
            game.world
                .units()
                .iter()
                .filter(|unit| unit.faction == CHOIR)
                .count(),
            choir_before
        );

        let canticle = game.world.unit_mut(canticle_id).unwrap();
        canticle.health = canticle.max_health * 0.5;
        assert!(game.update_boss_phase());
        assert_eq!(
            game.world
                .units()
                .iter()
                .filter(|unit| unit.faction == CHOIR)
                .count(),
            choir_before + 2
        );

        // Fires only once even if health stays low on later ticks.
        assert!(!game.update_boss_phase());
    }

    #[test]
    fn player_move_routes_around_conduit_obstacles() {
        let mut game = LastLight::new();
        game.start_mission(missions::voice_in_conduit_twelve());
        let unit_id = game.world.units()[0].id;
        let start = game.world.units()[0].position;
        game.world.select_point(start, PLAYER, false);
        assert!(game.world.selection().contains(unit_id));

        let destination = Vec2::new(start.x, -200.0);
        assert!(
            game.nav.segment_blocked(start, destination),
            "test destination should cross a corridor wall"
        );

        game.world.issue_move(destination, 74.0);
        game.route_around_obstacles(&[unit_id]);

        let UnitOrder::Move(first_waypoint) = game.world.unit(unit_id).unwrap().order else {
            panic!("expected a Move order toward the first routed waypoint");
        };
        assert_ne!(
            first_waypoint, destination,
            "a blocked destination should route via an intermediate waypoint, not go straight there"
        );
        assert!(game.player_paths.contains_key(&unit_id));

        // Simulate arrival at the first waypoint and confirm the route continues.
        {
            let unit = game.world.unit_mut(unit_id).unwrap();
            unit.position = first_waypoint;
            unit.order = UnitOrder::Idle;
        }
        game.advance_player_paths();
        let order_after = game.world.unit(unit_id).unwrap().order;
        assert!(
            matches!(order_after, UnitOrder::Move(_)),
            "should advance to the next queued waypoint after arriving at the first"
        );
    }
}
