//! Aurora: Last Light — Reclaim the Reactor.
//! Point-and-click RTS vertical slice powered by Aurora Engine.

use std::collections::HashMap;

use aurora_engine::{
    run, Aabb, Animation, BitmapText, Color, FactionId, FogOfWar, FogState, FrameCtx, Game,
    PointLight, Renderer, RtsWorld, SelectionBox, Sprite, Texture, TextureAtlas, TextureHandle,
    UnitId, UnitOrder,
};
use glam::Vec2;
use winit::{event::MouseButton, keyboard::KeyCode};

const MAP_SIZE: Vec2 = Vec2::new(2600.0, 1460.0);
const PLAYER: FactionId = FactionId(1);
const CHOIR: FactionId = FactionId(2);
const UNIT_ATLAS_SIZE: Vec2 = Vec2::new(1536.0, 1024.0);
const STRUCTURE_ATLAS_SIZE: Vec2 = Vec2::splat(1254.0);

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

struct LastLight {
    tex_environment: TextureHandle,
    tex_units: TextureHandle,
    tex_warden_move: TextureHandle,
    tex_structures: TextureHandle,
    tex_glow: TextureHandle,
    tex_ui: TextureHandle,
    unit_atlas: TextureAtlas,
    warden_move_atlas: TextureAtlas,
    warden_move_animation: Animation,
    structure_atlas: TextureAtlas,
    world: RtsWorld,
    kinds: HashMap<UnitId, UnitKind>,
    attack_flash: HashMap<UnitId, f32>,
    fog: FogOfWar,
    drag: Option<SelectionBox>,
    order_marker: Option<(Vec2, f32)>,
    relays: Vec<Relay>,
    reactor_position: Vec2,
    briefing: bool,
    paused: bool,
    victory: bool,
    defeat: bool,
    enemy_think: f32,
    mission_time: f32,
}

impl LastLight {
    fn new() -> Self {
        let mut game = Self {
            tex_environment: TextureHandle::default(),
            tex_units: TextureHandle::default(),
            tex_warden_move: TextureHandle::default(),
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
            warden_move_animation: Animation::new([0, 1, 2, 3, 4, 5], 10.0),
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
        if let Some(unit) = self.world.unit_mut(id) {
            unit.health = health;
            unit.max_health = health;
            unit.speed = speed;
            unit.radius = kind.scale() * 0.27;
        }
        self.kinds.insert(id, kind);
        id
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
            self.drag = Some(SelectionBox::begin(mouse_world));
        }
        if let Some(drag) = self.drag.as_mut() {
            drag.update(mouse_world);
        }
        if ctx.input.mouse_released(MouseButton::Left) {
            if let Some(drag) = self.drag.take() {
                if drag.start.distance(drag.current) < 18.0 {
                    self.world.select_point(mouse_world, PLAYER, false);
                } else {
                    self.world.select_bounds(drag.bounds(), PLAYER, false);
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
                let dps = if self.kinds.get(&unit.id) == Some(&UnitKind::Warden) {
                    34.0
                } else {
                    18.0
                };
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
                440.0
            } else {
                300.0
            };
            self.fog.reveal(unit.position, radius);
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
        let (environment, units, warden_move, structures, glow, ui) = {
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
        self.tex_structures = renderer.add_texture(structures);
        self.tex_glow = renderer.add_texture(glow);
        self.tex_ui = renderer.add_texture(ui);
        self.unit_atlas = TextureAtlas::new(self.tex_units, 3, 2, UNIT_ATLAS_SIZE);
        self.warden_move_atlas =
            TextureAtlas::new(self.tex_warden_move, 6, 1, Vec2::new(2172.0, 724.0));
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
            if ctx.input.key_pressed(KeyCode::Space) || ctx.input.key_pressed(KeyCode::Enter) {
                self.briefing = false;
                ctx.audio.start();
            }
            return;
        }
        if ctx.input.key_pressed(KeyCode::Escape) {
            self.paused = !self.paused;
        }
        if self.paused || self.victory || self.defeat {
            return;
        }

        self.handle_pointer(ctx);
        self.update_enemy_ai(dt);
        self.world.update(dt);
        if self.world.units().iter().any(|unit| {
            unit.alive()
                && self.kinds.get(&unit.id) == Some(&UnitKind::Warden)
                && unit.velocity.length_squared() > 1.0
        }) {
            self.warden_move_animation.tick(dt);
        } else {
            self.warden_move_animation.reset();
        }
        self.update_combat(dt);
        self.update_fog();
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
                self.relays[index].progress += dt;
                if self.relays[index].progress >= 3.0 {
                    self.relays[index].progress = 3.0;
                    self.relays[index].active = true;
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
            let (texture, mut sprite) =
                if kind == UnitKind::Warden && unit.velocity.length_squared() > 1.0 {
                    (
                        self.tex_warden_move,
                        self.warden_move_atlas.sprite(
                            unit.position,
                            Vec2::splat(kind.scale()),
                            self.warden_move_animation.frame(),
                        ),
                    )
                } else {
                    (
                        self.tex_units,
                        self.unit_atlas.sprite(
                            unit.position,
                            Vec2::splat(kind.scale()),
                            kind.atlas_frame(),
                        ),
                    )
                };
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
            "DRAG SELECT  RIGHT CLICK COMMAND  WASD PAN  WHEEL ZOOM",
            top_left + Vec2::new(0.0, -25.0),
            2.5,
            Color::rgba(0.58, 0.7, 0.78, 0.86),
            8.0,
        );
        if let Some(selected) = self.world.selection().ids().first() {
            self.draw_text(
                ctx.renderer,
                self.kinds[selected].label(),
                top_left + Vec2::new(0.0, -50.0),
                3.2,
                Color::rgb(0.96, 0.72, 0.28),
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
                "MISSION COMPLETE",
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
                Sprite::new(center, Vec2::new((view.x * 0.78).min(900.0), 300.0))
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
                2.7,
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
        }
    }
}

fn main() {
    run(LastLight::new());
}
