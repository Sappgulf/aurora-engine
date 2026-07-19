//! Aurora: Last Light — Reclaim the Reactor.
//! Point-and-click RTS vertical slice powered by Aurora Engine.

use std::collections::HashMap;

use aurora_engine::{
    run, Aabb, AnimationClip, AnimationPlayer, BitmapText, Color, FactionId, FogOfWar, FogState,
    FrameCtx, Game, MinimapTransform, PlacementError, PlacementRules, PointLight, PowerGrid,
    PowerNode, PowerNodeId, ProductId, ProductionQueue, ProductionRecipe, QueueError, Renderer,
    ResourceBank, RtsWorld, SaveData, SaveStore, SelectionBox, Sprite, Texture, TextureAtlas,
    TextureHandle, UnitId, UnitOrder,
};
use glam::Vec2;
use winit::{event::MouseButton, keyboard::KeyCode};

const MAP_SIZE: Vec2 = Vec2::new(2600.0, 1460.0);
const PLAYER: FactionId = FactionId(1);
const CHOIR: FactionId = FactionId(2);
const UNIT_ATLAS_SIZE: Vec2 = Vec2::new(1536.0, 1024.0);
const STRUCTURE_ATLAS_SIZE: Vec2 = Vec2::splat(1254.0);
const FABRICATOR_NODE: PowerNodeId = PowerNodeId(0);
const WARDEN_PRODUCT: ProductId = ProductId(0);
const ENGINEER_PRODUCT: ProductId = ProductId(1);
const SURVEYOR_PRODUCT: ProductId = ProductId(2);
const BEACON_COST: u32 = 50;
const UPGRADE_OPTICS: &str = "field-optics";
const UPGRADE_PLATING: &str = "reactive-plating";
const UPGRADE_OVERCLOCK: &str = "fabricator-overclock";
const IVO: &str = "ivo-rook";
const SENA: &str = "sena-quill";
const IVO_RIGGER: &str = "relay-rigger";
const IVO_SMITH: &str = "salvage-smith";
const SENA_DEEP_SCAN: &str = "deep-scan";
const SENA_GHOST_MARK: &str = "ghost-mark";
const MARA: &str = "mara-vey";
const MARA_RESCUE: &str = "rescue-screen";
const MARA_RAPID: &str = "rapid-command";
const OLAN: &str = "olan-voss";
const OLAN_LATTICE: &str = "lattice-audit";
const OLAN_DECODER: &str = "choir-decoder";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitKind {
    Warden,
    Engineer,
    Surveyor,
    Needle,
    Canticle,
    BellMine,
}

impl UnitKind {
    fn from_product(product: ProductId) -> Option<Self> {
        match product {
            WARDEN_PRODUCT => Some(Self::Warden),
            ENGINEER_PRODUCT => Some(Self::Engineer),
            SURVEYOR_PRODUCT => Some(Self::Surveyor),
            _ => None,
        }
    }

    fn recipe(self) -> Option<ProductionRecipe> {
        match self {
            Self::Warden => Some(ProductionRecipe::new(WARDEN_PRODUCT, 90, 6_000)),
            Self::Engineer => Some(ProductionRecipe::new(ENGINEER_PRODUCT, 70, 5_000)),
            Self::Surveyor => Some(ProductionRecipe::new(SURVEYOR_PRODUCT, 60, 4_000)),
            Self::Needle | Self::Canticle | Self::BellMine => None,
        }
    }

    fn atlas_frame(self) -> u32 {
        match self {
            Self::Warden => 0,
            Self::Engineer => 1,
            Self::Surveyor => 2,
            Self::Needle => 3,
            Self::Canticle => 4,
            Self::BellMine => 5,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Warden => "WARDEN",
            Self::Engineer => "ENGINEER",
            Self::Surveyor => "SURVEYOR",
            Self::Needle => "CHOIR NEEDLE",
            Self::Canticle => "CHOIR CANTICLE",
            Self::BellMine => "BELL MINE",
        }
    }

    fn scale(self) -> f32 {
        match self {
            Self::Warden => 116.0,
            Self::Engineer => 108.0,
            Self::Surveyor => 105.0,
            Self::Needle => 104.0,
            Self::Canticle => 116.0,
            Self::BellMine => 96.0,
        }
    }
}

struct Relay {
    position: Vec2,
    progress: f32,
    active: bool,
}

struct FieldBeacon {
    position: Vec2,
}

struct LastLight {
    tex_environment: TextureHandle,
    tex_units: TextureHandle,
    tex_warden_move: TextureHandle,
    tex_engineer_move: TextureHandle,
    tex_surveyor_scan: TextureHandle,
    tex_needle_attack: TextureHandle,
    tex_canticle_command: TextureHandle,
    tex_bell_mine_arm: TextureHandle,
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
    animation_players: HashMap<UnitId, AnimationPlayer>,
    structure_atlas: TextureAtlas,
    world: RtsWorld,
    kinds: HashMap<UnitId, UnitKind>,
    attack_flash: HashMap<UnitId, f32>,
    fog: FogOfWar,
    drag: Option<SelectionBox>,
    order_marker: Option<(Vec2, f32)>,
    relays: Vec<Relay>,
    reactor_position: Vec2,
    fabricator_position: Vec2,
    field_beacons: Vec<FieldBeacon>,
    placing_beacon: bool,
    resources: ResourceBank,
    resource_tick: f32,
    production: ProductionQueue,
    power: PowerGrid,
    status: Option<(String, f32)>,
    save_store: SaveStore,
    save_data: SaveData,
    victory_saved: bool,
    briefing: bool,
    paused: bool,
    victory: bool,
    defeat: bool,
    enemy_think: f32,
    mission_time: f32,
}

impl LastLight {
    fn new() -> Self {
        let save_store = SaveStore::new("last-light-campaign");
        let save_data = save_store.load().ok().flatten().unwrap_or_default();
        let starting_salvage = 150_u32.saturating_add(save_data.campaign.currency.min(100) as u32);
        let mut power = PowerGrid::default();
        power.add_node(PowerNode {
            id: FABRICATOR_NODE,
            supply: 1,
            demand: 1,
            online: true,
        });
        for index in 0..3 {
            let relay = PowerNodeId(index + 1);
            power.add_node(PowerNode {
                id: relay,
                supply: 1,
                demand: 0,
                online: false,
            });
            power.link(FABRICATOR_NODE, relay);
        }
        let mut game = Self {
            tex_environment: TextureHandle::default(),
            tex_units: TextureHandle::default(),
            tex_warden_move: TextureHandle::default(),
            tex_engineer_move: TextureHandle::default(),
            tex_surveyor_scan: TextureHandle::default(),
            tex_needle_attack: TextureHandle::default(),
            tex_canticle_command: TextureHandle::default(),
            tex_bell_mine_arm: TextureHandle::default(),
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
            fog: FogOfWar::new(26, 15, -MAP_SIZE * 0.5, 100.0),
            drag: None,
            order_marker: None,
            relays: vec![
                Relay {
                    position: Vec2::new(-790.0, 320.0),
                    progress: 0.0,
                    active: false,
                },
                Relay {
                    position: Vec2::new(30.0, -430.0),
                    progress: 0.0,
                    active: false,
                },
                Relay {
                    position: Vec2::new(830.0, 250.0),
                    progress: 0.0,
                    active: false,
                },
            ],
            reactor_position: Vec2::new(520.0, -40.0),
            fabricator_position: Vec2::new(-1_020.0, -120.0),
            field_beacons: Vec::new(),
            placing_beacon: false,
            resources: ResourceBank::new(starting_salvage),
            resource_tick: 0.0,
            production: ProductionQueue::new(5),
            power,
            status: Some(("FABRICATOR READY — Q/E/F TO BUILD".to_owned(), 7.0)),
            save_store,
            save_data,
            victory_saved: false,
            briefing: true,
            paused: false,
            victory: false,
            defeat: false,
            enemy_think: 0.0,
            mission_time: 0.0,
        };
        game.populate_mission();
        game
    }

    fn populate_mission(&mut self) {
        self.spawn(
            UnitKind::Warden,
            PLAYER,
            Vec2::new(-880.0, -290.0),
            155.0,
            175.0,
        );
        self.spawn(
            UnitKind::Engineer,
            PLAYER,
            Vec2::new(-790.0, -350.0),
            115.0,
            150.0,
        );
        self.spawn(
            UnitKind::Surveyor,
            PLAYER,
            Vec2::new(-900.0, -410.0),
            90.0,
            215.0,
        );

        for (kind, position) in [
            (UnitKind::Needle, Vec2::new(-480.0, 250.0)),
            (UnitKind::BellMine, Vec2::new(-120.0, -330.0)),
            (UnitKind::Needle, Vec2::new(290.0, 290.0)),
            (UnitKind::BellMine, Vec2::new(650.0, -310.0)),
            (UnitKind::Needle, Vec2::new(930.0, 390.0)),
            (UnitKind::Canticle, Vec2::new(520.0, 40.0)),
        ] {
            let health = if kind == UnitKind::Canticle {
                340.0
            } else {
                90.0
            };
            self.spawn(
                kind,
                CHOIR,
                position,
                health,
                if kind == UnitKind::BellMine {
                    75.0
                } else {
                    125.0
                },
            );
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
        self.status = Some(match self.save_store.save(&self.save_data) {
            Ok(()) => (format!("{label} INSTALLED"), 3.5),
            Err(error) => (format!("UPGRADE SAVE FAILED: {error}"), 5.0),
        });
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
        self.status = Some(match self.save_store.save(&self.save_data) {
            Ok(()) => (format!("{label} LOADOUT: {}", next.to_uppercase()), 3.5),
            Err(error) => (format!("LOADOUT SAVE FAILED: {error}"), 5.0),
        });
    }

    fn beacon_cost(&self) -> u32 {
        if self.specialist_module(IVO, IVO_RIGGER) == IVO_SMITH {
            40
        } else {
            BEACON_COST
        }
    }

    fn relay_income(&self) -> u32 {
        if self.specialist_module(OLAN, OLAN_LATTICE) == OLAN_LATTICE {
            4
        } else {
            3
        }
    }

    fn handle_briefing_upgrades(&mut self, ctx: &FrameCtx<'_>) {
        if ctx.input.key_pressed(KeyCode::KeyZ) {
            self.purchase_upgrade(UPGRADE_OPTICS, "FIELD OPTICS", 60);
        }
        if ctx.input.key_pressed(KeyCode::KeyX) {
            self.purchase_upgrade(UPGRADE_PLATING, "REACTIVE PLATING", 80);
        }
        if ctx.input.key_pressed(KeyCode::KeyC) {
            self.purchase_upgrade(UPGRADE_OVERCLOCK, "FABRICATOR OVERCLOCK", 100);
        }
        if ctx.input.key_pressed(KeyCode::KeyV) {
            self.cycle_specialist(IVO, IVO_RIGGER, IVO_SMITH, "IVO");
        }
        if ctx.input.key_pressed(KeyCode::KeyN) {
            self.cycle_specialist(SENA, SENA_DEEP_SCAN, SENA_GHOST_MARK, "SENA");
        }
        if ctx.input.key_pressed(KeyCode::KeyM) {
            self.cycle_specialist(MARA, MARA_RESCUE, MARA_RAPID, "MARA");
        }
        if ctx.input.key_pressed(KeyCode::KeyO) {
            self.cycle_specialist(OLAN, OLAN_LATTICE, OLAN_DECODER, "OLAN");
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
        let mut obstructions = vec![
            (self.fabricator_position, 105.0),
            (self.reactor_position, 135.0),
        ];
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

    fn handle_command_keys(&mut self, ctx: &mut FrameCtx<'_>) {
        for (key, kind) in [
            (KeyCode::KeyQ, UnitKind::Warden),
            (KeyCode::KeyE, UnitKind::Engineer),
            (KeyCode::KeyF, UnitKind::Surveyor),
        ] {
            if ctx.input.key_pressed(key) {
                self.queue_unit(kind);
            }
        }
        if ctx.input.key_pressed(KeyCode::KeyH) {
            self.world.issue_hold();
            self.status = Some(("SQUAD HOLDING POSITION".to_owned(), 2.0));
        }
        if ctx.input.key_pressed(KeyCode::KeyB) {
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

        for (slot, key) in [
            (1, KeyCode::Digit1),
            (2, KeyCode::Digit2),
            (3, KeyCode::Digit3),
            (4, KeyCode::Digit4),
            (5, KeyCode::Digit5),
        ] {
            if !ctx.input.key_pressed(key) {
                continue;
            }
            if ctx.input.control_down() {
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
        self.save_data
            .campaign
            .complete_mission("reclaim-the-reactor", 3, 80);
        self.save_data
            .campaign
            .record_decision("lumen-contact-established");
        self.status = Some(match self.save_store.save(&self.save_data) {
            Ok(()) => ("CAMPAIGN SAVED — MISSION 3 UNLOCKED".to_owned(), 8.0),
            Err(error) => (format!("SAVE FAILED: {error}"), 8.0),
        });
        self.victory_saved = true;
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
                    self.world
                        .select_point(mouse_world, PLAYER, ctx.input.shift_down());
                } else {
                    self.world
                        .select_bounds(drag.bounds(), PLAYER, ctx.input.shift_down());
                }
            }
        }
        if ctx.input.mouse_pressed(MouseButton::Right) && !self.world.selection().ids().is_empty() {
            if let Some(enemy) = self.closest_enemy_at(mouse_world) {
                self.world.issue_attack(enemy);
            } else {
                self.world.issue_move(mouse_world, 74.0);
            }
            self.order_marker = Some((mouse_world, 0.65));
            ctx.audio.collect();
        }
    }

    fn update_camera(&mut self, ctx: &mut FrameCtx<'_>, dt: f32) {
        let viewport = ctx.renderer.camera.viewport();
        let mouse = ctx.input.mouse_position;
        let mut pan = ctx.input.axis_wasd();
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
        let friendlies: Vec<(UnitId, Vec2)> = self
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == PLAYER && unit.alive())
            .map(|unit| (unit.id, unit.position))
            .collect();
        let enemies: Vec<(UnitId, Vec2)> = self
            .world
            .units()
            .iter()
            .filter(|unit| unit.faction == CHOIR && unit.alive())
            .map(|unit| (unit.id, unit.position))
            .collect();
        for (enemy, position) in enemies {
            let target = friendlies
                .iter()
                .filter(|(_, friendly_position)| {
                    friendly_position.distance_squared(position) <= 520.0_f32.powi(2)
                })
                .min_by(|a, b| {
                    a.1.distance_squared(position)
                        .total_cmp(&b.1.distance_squared(position))
                })
                .map(|(id, _)| *id);
            if let (Some(target), Some(unit)) = (target, self.world.unit_mut(enemy)) {
                unit.order = UnitOrder::Attack(target);
            }
        }
    }

    fn unit_engaged(&self, id: UnitId, range: f32) -> bool {
        let Some(unit) = self.world.unit(id) else {
            return false;
        };
        let UnitOrder::Attack(target) = unit.order else {
            return false;
        };
        self.world.unit(target).is_some_and(|target| {
            target.alive() && unit.position.distance(target.position) <= range
        })
    }

    fn update_specialist_doctrines(&mut self, dt: f32) {
        if self.specialist_module(MARA, MARA_RESCUE) != MARA_RESCUE {
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
                unit.health = (unit.health + dt.max(0.0) * 3.0).min(unit.max_health);
            }
        }
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
            if unit.position.distance(target_position) < 125.0 {
                let mut dps = match self.kinds.get(&unit.id) {
                    Some(UnitKind::Warden) => 34.0,
                    Some(UnitKind::Canticle) => 24.0,
                    Some(UnitKind::BellMine) => 28.0,
                    _ => 18.0,
                };
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
        for (target, amount) in damage {
            if let Some(unit) = self.world.unit_mut(target) {
                unit.health = (unit.health - amount).max(0.0);
            }
        }
        self.attack_flash.retain(|_, flash| {
            *flash -= dt;
            *flash > 0.0
        });
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
                if self.specialist_module(SENA, SENA_DEEP_SCAN) == SENA_DEEP_SCAN {
                    540.0
                } else {
                    440.0
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
        let (
            environment,
            units,
            warden_move,
            engineer_move,
            surveyor_scan,
            needle_attack,
            canticle_command,
            bell_mine_arm,
            structures,
            glow,
            ui,
        ) = {
            let gpu = renderer.gpu();
            (
                Texture::from_bytes(
                    &gpu,
                    include_bytes!("../assets/reactor-sector-v001.png"),
                    "Last Light reactor sector",
                )
                .expect("reactor sector must decode"),
                Texture::from_bytes(
                    &gpu,
                    include_bytes!("../assets/last-light-units-atlas-v001.png"),
                    "Last Light units",
                )
                .expect("unit atlas must decode"),
                Texture::from_bytes(
                    &gpu,
                    include_bytes!("../assets/warden-move-strip-v001.png"),
                    "Warden move animation",
                )
                .expect("Warden animation must decode"),
                Texture::from_bytes(
                    &gpu,
                    include_bytes!("../assets/engineer-move-strip-v001.png"),
                    "Engineer move animation",
                )
                .expect("Engineer animation must decode"),
                Texture::from_bytes(
                    &gpu,
                    include_bytes!("../assets/surveyor-scan-strip-v001.png"),
                    "Surveyor scan animation",
                )
                .expect("Surveyor animation must decode"),
                Texture::from_bytes(
                    &gpu,
                    include_bytes!("../assets/needle-attack-strip-v001.png"),
                    "Choir Needle attack animation",
                )
                .expect("Needle animation must decode"),
                Texture::from_bytes(
                    &gpu,
                    include_bytes!("../assets/canticle-command-strip-v001.png"),
                    "Choir Canticle command animation",
                )
                .expect("Canticle animation must decode"),
                Texture::from_bytes(
                    &gpu,
                    include_bytes!("../assets/bell-mine-arm-strip-v001.png"),
                    "Choir Bell Mine arming animation",
                )
                .expect("Bell Mine animation must decode"),
                Texture::from_bytes(
                    &gpu,
                    include_bytes!("../assets/last-light-structures-atlas-v001.png"),
                    "Last Light structures",
                )
                .expect("structure atlas must decode"),
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
        if self.briefing {
            self.handle_briefing_upgrades(ctx);
            if ctx.input.key_pressed(KeyCode::Space) || ctx.input.key_pressed(KeyCode::Enter) {
                self.briefing = false;
                ctx.audio.start();
            }
            return;
        }
        if ctx.input.key_pressed(KeyCode::Escape) {
            if self.placing_beacon {
                self.placing_beacon = false;
                self.status = Some(("BEACON PLACEMENT CANCELLED".to_owned(), 2.0));
            } else {
                self.paused = !self.paused;
            }
        }
        if self.paused || self.victory || self.defeat {
            return;
        }

        self.handle_command_keys(ctx);
        self.handle_pointer(ctx);
        self.update_enemy_ai(dt);
        self.world.update(dt);
        for unit in self.world.units().iter().filter(|unit| unit.alive()) {
            let kind = self.kinds.get(&unit.id).copied();
            let engaged = match kind {
                Some(UnitKind::Needle) => self.unit_engaged(unit.id, 155.0),
                Some(UnitKind::Canticle) => self.unit_engaged(unit.id, 260.0),
                Some(UnitKind::BellMine) => self.unit_engaged(unit.id, 220.0),
                _ => false,
            };
            let Some(player) = self.animation_players.get_mut(&unit.id) else {
                continue;
            };
            let clip = match kind {
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
            };
            if let Some(clip) = clip {
                player.play(clip);
                player.tick(dt);
            }
        }
        self.update_combat(dt);
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

        let friendlies_alive = self
            .world
            .units()
            .iter()
            .any(|unit| unit.faction == PLAYER && unit.alive());
        let cantor_alive = self.world.units().iter().any(|unit| {
            unit.faction == CHOIR
                && unit.alive()
                && self.kinds.get(&unit.id) == Some(&UnitKind::Canticle)
        });
        self.defeat = !friendlies_alive;
        self.victory = self.relays.iter().all(|relay| relay.active) && !cantor_alive;
        if self.victory {
            self.persist_victory();
        }
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let t = ctx.time.elapsed;
        ctx.renderer.draw_sprite(
            self.tex_environment,
            Sprite::new(Vec2::ZERO, MAP_SIZE).with_z(-10.0),
        );

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

        let reactor_pulse = 0.55 + 0.12 * (t * 2.1).sin();
        let mut reactor = self
            .structure_atlas
            .sprite(self.reactor_position, Vec2::splat(330.0), 2);
        reactor.z = -1.0;
        ctx.renderer.draw_sprite(self.tex_structures, reactor);
        ctx.renderer.draw_light(PointLight::new(
            self.reactor_position,
            Color::rgb(0.16, 0.58, 0.8),
            260.0,
            reactor_pulse * 0.26,
        ));

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
            if !unit.alive() {
                continue;
            }
            if unit.faction == CHOIR && self.fog.state_at(unit.position) != FogState::Visible {
                continue;
            }
            let kind = self.kinds[&unit.id];
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
            let frame = self
                .animation_players
                .get(&unit.id)
                .map(AnimationPlayer::frame)
                .unwrap_or(0);
            let engaged = match kind {
                UnitKind::Needle => self.unit_engaged(unit.id, 155.0),
                UnitKind::Canticle => self.unit_engaged(unit.id, 260.0),
                UnitKind::BellMine => self.unit_engaged(unit.id, 220.0),
                _ => false,
            };
            let animated = match kind {
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
        }

        let top_left = ctx
            .renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(0.0, 1.0))
            + Vec2::new(30.0, -34.0);
        let active_relays = self.relays.iter().filter(|relay| relay.active).count();
        self.draw_text(
            ctx.renderer,
            &format!("RECLAIM REACTOR  RELAYS {active_relays}/3"),
            top_left,
            4.0,
            Color::rgb(0.73, 1.15, 1.08),
            8.0,
        );
        self.draw_text(
            ctx.renderer,
            "SHIFT ADD  RIGHT CLICK COMMAND  B BUILD BEACON  WHEEL ZOOM",
            top_left + Vec2::new(0.0, -25.0),
            2.5,
            Color::rgba(0.58, 0.7, 0.78, 0.86),
            8.0,
        );
        let income = active_relays * self.relay_income() as usize;
        self.draw_text(
            ctx.renderer,
            &format!(
                "SALVAGE {}  +{income}/S  POWER {}/4  UNITS {}/12",
                self.resources.amount(),
                active_relays + 1,
                self.friendly_count()
            ),
            top_left + Vec2::new(0.0, -50.0),
            2.8,
            Color::rgb(0.96, 0.72, 0.28),
            8.0,
        );
        if let Some(selected) = self.world.selection().ids().first() {
            self.draw_text(
                ctx.renderer,
                self.kinds[selected].label(),
                top_left + Vec2::new(0.0, -75.0),
                3.2,
                Color::rgb(0.96, 0.72, 0.28),
                8.0,
            );
        }
        if let Some((message, _)) = &self.status {
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
            let bottom_right = ctx
                .renderer
                .camera
                .world_from_viewport_fraction(Vec2::new(1.0, 0.0));
            let card_center = bottom_right + Vec2::new(-285.0, 120.0);
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(card_center, Vec2::new(530.0, 210.0))
                    .with_color(Color::rgba(0.01, 0.025, 0.05, 0.88))
                    .with_z(7.5),
            );
            let card_text = bottom_right + Vec2::new(-525.0, 205.0);
            self.draw_text(
                ctx.renderer,
                "LANTERN FABRICATOR",
                card_text,
                2.8,
                Color::rgb(0.3, 1.4, 1.2),
                8.0,
            );
            self.draw_text(
                ctx.renderer,
                "Q WARDEN 90   E ENGINEER 70",
                card_text + Vec2::new(0.0, -38.0),
                2.0,
                Color::rgb(0.88, 0.92, 0.92),
                8.0,
            );
            self.draw_text(
                ctx.renderer,
                &format!("F SURVEYOR 60   H HOLD   B BEACON {}", self.beacon_cost()),
                card_text + Vec2::new(0.0, -70.0),
                2.0,
                Color::rgb(0.88, 0.92, 0.92),
                8.0,
            );
            self.draw_text(
                ctx.renderer,
                "CMD/CTRL+1-5 ASSIGN   1-5 RECALL",
                card_text + Vec2::new(0.0, -102.0),
                1.7,
                Color::rgba(0.55, 0.7, 0.78, 0.9),
                8.0,
            );
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
                card_text + Vec2::new(0.0, -138.0),
                2.0,
                Color::rgb(1.15, 0.7, 0.25),
                8.0,
            );
        }

        let center = ctx.renderer.camera.position;
        let view = ctx.renderer.camera.visible_world_size();
        let overlay = if self.briefing {
            Some((
                "RECLAIM THE REACTOR",
                "MARA VEY: FIND IVO. RESTORE THREE RELAYS. SILENCE THE CHOIR.",
                "SPACE TO DEPLOY",
            ))
        } else if self.paused {
            Some(("TACTICAL PAUSE", "ORDERS SUSPENDED", "ESC TO RESUME"))
        } else if self.victory {
            Some((
                "REACTOR ONLINE",
                "LUMEN: I CAN SEE YOU NOW, COMMANDER.",
                "CAMPAIGN SAVED — MISSION 3 UNLOCKED",
            ))
        } else if self.defeat {
            Some((
                "LANTERN LOST",
                "THE DARK CLOSES OVER CONDUIT TWELVE.",
                "RESTART THE GAME TO RETRY",
            ))
        } else {
            None
        };
        if let Some((title, story, prompt)) = overlay {
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    center,
                    Vec2::new(
                        (view.x * 0.78).min(900.0),
                        if self.briefing { 420.0 } else { 300.0 },
                    ),
                )
                .with_color(Color::rgba(0.012, 0.025, 0.055, 0.92))
                .with_z(10.0),
            );
            self.draw_text(
                ctx.renderer,
                title,
                center + Vec2::new(-300.0, 75.0),
                7.0,
                Color::rgb(0.28, 1.5, 1.3),
                11.0,
            );
            self.draw_text(
                ctx.renderer,
                story,
                center + Vec2::new(-330.0, 5.0),
                2.0,
                Color::rgb(0.78, 0.88, 0.9),
                11.0,
            );
            self.draw_text(
                ctx.renderer,
                prompt,
                center + Vec2::new(-150.0, -72.0),
                3.5,
                Color::rgb(1.25, 0.74, 0.24),
                11.0,
            );
            if self.briefing {
                let owned = |id| {
                    if self.save_data.campaign.has_upgrade(id) {
                        "INSTALLED"
                    } else {
                        "AVAILABLE"
                    }
                };
                self.draw_text(
                    ctx.renderer,
                    &format!(
                        "LUMEN {}  Z OPTICS 60 {}  X PLATING 80 {}  C OVERCLOCK 100 {}",
                        self.save_data.campaign.currency,
                        owned(UPGRADE_OPTICS),
                        owned(UPGRADE_PLATING),
                        owned(UPGRADE_OVERCLOCK)
                    ),
                    center + Vec2::new(-410.0, -122.0),
                    1.65,
                    Color::rgba(0.55, 0.82, 0.88, 0.94),
                    11.0,
                );
                self.draw_text(
                    ctx.renderer,
                    &format!(
                        "V IVO {}  N SENA {}",
                        self.specialist_module(IVO, IVO_RIGGER).to_uppercase(),
                        self.specialist_module(SENA, SENA_DEEP_SCAN).to_uppercase()
                    ),
                    center + Vec2::new(-245.0, -154.0),
                    1.8,
                    Color::rgba(0.82, 0.68, 0.36, 0.98),
                    11.0,
                );
                self.draw_text(
                    ctx.renderer,
                    &format!(
                        "M MARA {}  O OLAN {}",
                        self.specialist_module(MARA, MARA_RESCUE).to_uppercase(),
                        self.specialist_module(OLAN, OLAN_LATTICE).to_uppercase()
                    ),
                    center + Vec2::new(-265.0, -184.0),
                    1.8,
                    Color::rgba(0.7, 0.62, 0.9, 0.98),
                    11.0,
                );
            }
        }
    }
}

fn main() {
    run(LastLight::new());
}
