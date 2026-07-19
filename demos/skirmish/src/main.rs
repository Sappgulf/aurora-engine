//! Skirmish: a minimal two-base RTS free-play mode built entirely from
//! `aurora-engine`'s generic RTS primitives (`RtsWorld`, `PowerGrid`,
//! `ProductionQueue`, `ResourceBank`, `SimpleAggroAi`) — no campaign save,
//! no briefing UI, no authored art. Destroy the enemy base's garrison to win.

use aurora_engine::{
    run, Aabb, AiParams, Color, FactionId, FrameCtx, Game, PointLight, PowerGrid, PowerNode,
    PowerNodeId, ProductId, ProductionQueue, ProductionRecipe, QueueError, Renderer, ResourceBank,
    RtsWorld, SelectionBox, SimpleAggroAi, Sprite, Texture, TextureHandle, UnitOrder,
};
use glam::Vec2;
use winit::{event::MouseButton, keyboard::KeyCode};

const MAP_SIZE: Vec2 = Vec2::new(2000.0, 1200.0);
const PLAYER: FactionId = FactionId(1);
const ENEMY: FactionId = FactionId(2);
const PLAYER_CORE: PowerNodeId = PowerNodeId(0);
const ENEMY_CORE: PowerNodeId = PowerNodeId(1);
const TROOPER: ProductId = ProductId(0);
const PLAYER_BASE: Vec2 = Vec2::new(-750.0, -420.0);
const ENEMY_BASE: Vec2 = Vec2::new(750.0, 420.0);
const UNIT_RADIUS: f32 = 22.0;
const ATTACK_RANGE: f32 = 90.0;
const ATTACK_DPS: f32 = 22.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Difficulty {
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    const ALL: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard];

    fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "EASY — SHORT-SIGHTED, CAUTIOUS PATROLS",
            Difficulty::Normal => "NORMAL — BALANCED AGGRESSION",
            Difficulty::Hard => "HARD — WIDE SENSORS, RELENTLESS FOCUS FIRE",
        }
    }

    fn ai_params(self) -> AiParams {
        match self {
            Difficulty::Easy => AiParams {
                aggro_radius: 380.0,
                retreat_health_fraction: 0.4,
                max_attackers_per_target: 1,
                ..AiParams::default()
            },
            Difficulty::Normal => AiParams::default(),
            Difficulty::Hard => AiParams {
                aggro_radius: 620.0,
                retreat_health_fraction: 0.15,
                max_attackers_per_target: 3,
                ..AiParams::default()
            },
        }
    }

    fn spawn_interval(self) -> f32 {
        match self {
            Difficulty::Easy => 13.0,
            Difficulty::Normal => 9.0,
            Difficulty::Hard => 6.0,
        }
    }
}

struct Skirmish {
    tex_player: TextureHandle,
    tex_enemy: TextureHandle,
    tex_base: TextureHandle,
    tex_ui: TextureHandle,
    world: RtsWorld,
    resources: ResourceBank,
    resource_tick: f32,
    production: ProductionQueue,
    power: PowerGrid,
    enemy_ai: SimpleAggroAi,
    drag: Option<SelectionBox>,
    order_marker: Option<(Vec2, f32)>,
    status: Option<(String, f32)>,
    victory: bool,
    defeat: bool,
    match_time: f32,
    enemy_spawn_timer: f32,
    enemy_think: f32,
    mode_select: bool,
    mode_cursor: usize,
    difficulty: Difficulty,
}

impl Default for Skirmish {
    fn default() -> Self {
        let mut power = PowerGrid::default();
        power.add_node(PowerNode {
            id: PLAYER_CORE,
            supply: 1,
            demand: 0,
            online: true,
        });
        power.add_node(PowerNode {
            id: ENEMY_CORE,
            supply: 1,
            demand: 0,
            online: true,
        });

        Self {
            tex_player: TextureHandle::default(),
            tex_enemy: TextureHandle::default(),
            tex_base: TextureHandle::default(),
            tex_ui: TextureHandle::default(),
            world: RtsWorld::default(),
            resources: ResourceBank::new(120),
            resource_tick: 0.0,
            production: ProductionQueue::new(5),
            power,
            enemy_ai: SimpleAggroAi::new(),
            drag: None,
            order_marker: None,
            status: None,
            victory: false,
            defeat: false,
            match_time: 0.0,
            enemy_spawn_timer: 8.0,
            enemy_think: 0.0,
            mode_select: true,
            mode_cursor: 1,
            difficulty: Difficulty::Normal,
        }
    }
}

impl Skirmish {
    fn start_match(&mut self, difficulty: Difficulty) {
        self.difficulty = difficulty;
        self.world = RtsWorld::default();
        for offset in [
            Vec2::new(-60.0, 0.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(60.0, 0.0),
        ] {
            self.world.spawn(PLAYER, PLAYER_BASE + offset);
        }
        for offset in [
            Vec2::new(-60.0, 0.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(60.0, 0.0),
        ] {
            self.world.spawn(ENEMY, ENEMY_BASE + offset);
        }
        self.resources = ResourceBank::new(120);
        self.resource_tick = 0.0;
        self.production = ProductionQueue::new(5);
        self.enemy_ai = SimpleAggroAi::new();
        self.status = Some((
            "DRAG SELECT   RIGHT CLICK MOVE/ATTACK   Q BUILD TROOPER (40)".to_owned(),
            6.0,
        ));
        self.victory = false;
        self.defeat = false;
        self.match_time = 0.0;
        self.enemy_spawn_timer = difficulty.spawn_interval();
        self.enemy_think = 0.0;
        self.mode_select = false;
    }

    fn mode_entry_rect(camera_position: Vec2, index: usize) -> Aabb {
        let center = camera_position + Vec2::new(0.0, 60.0 - index as f32 * 74.0);
        Aabb::from_center_size(center, Vec2::new(760.0, 62.0))
    }

    fn handle_mode_select(&mut self, ctx: &mut FrameCtx<'_>) {
        if ctx.input.key_pressed(KeyCode::ArrowUp) || ctx.input.key_pressed(KeyCode::ArrowLeft) {
            self.mode_cursor =
                (self.mode_cursor + Difficulty::ALL.len() - 1) % Difficulty::ALL.len();
        }
        if ctx.input.key_pressed(KeyCode::ArrowDown) || ctx.input.key_pressed(KeyCode::ArrowRight) {
            self.mode_cursor = (self.mode_cursor + 1) % Difficulty::ALL.len();
        }
        let mut confirmed =
            ctx.input.key_pressed(KeyCode::Space) || ctx.input.key_pressed(KeyCode::Enter);
        if ctx.input.mouse_pressed(MouseButton::Left) {
            let mouse_world = ctx
                .renderer
                .camera
                .screen_to_world(ctx.input.mouse_position);
            for index in 0..Difficulty::ALL.len() {
                if Self::mode_entry_rect(ctx.renderer.camera.position, index)
                    .contains_point(mouse_world)
                {
                    self.mode_cursor = index;
                    confirmed = true;
                    break;
                }
            }
        }
        if confirmed {
            self.start_match(Difficulty::ALL[self.mode_cursor]);
        }
    }

    fn draw_mode_select(&self, ctx: &mut FrameCtx<'_>) {
        let center = ctx.renderer.camera.position;
        let view = ctx.renderer.camera.visible_world_size();
        ctx.renderer.draw_sprite(
            self.tex_ui,
            Sprite::new(center, view * 1.05)
                .with_color(Color::rgba(0.01, 0.015, 0.03, 0.97))
                .with_z(9.0),
        );
        self.draw_text(
            ctx.renderer,
            "AURORA ENGINE — SKIRMISH",
            center + Vec2::new(-320.0, 260.0),
            6.0,
            Color::rgb(0.32, 1.4, 1.55),
        );
        self.draw_text(
            ctx.renderer,
            "SELECT OPPONENT DIFFICULTY",
            center + Vec2::new(-320.0, 200.0),
            2.8,
            Color::rgba(0.75, 0.85, 0.9, 0.95),
        );
        for (index, difficulty) in Difficulty::ALL.iter().enumerate() {
            let rect = Self::mode_entry_rect(center, index);
            let hovered = index == self.mode_cursor;
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(rect.center(), rect.size())
                    .with_color(if hovered {
                        Color::rgba(0.16, 0.55, 0.6, 0.55)
                    } else {
                        Color::rgba(0.05, 0.09, 0.14, 0.55)
                    })
                    .with_z(9.5),
            );
            let marker = if hovered { ">" } else { " " };
            self.draw_text(
                ctx.renderer,
                &format!("{marker} {}", difficulty.label()),
                rect.min + Vec2::new(24.0, 22.0),
                2.6,
                if hovered {
                    Color::rgb(1.3, 0.95, 0.35)
                } else {
                    Color::rgb(0.8, 0.9, 0.92)
                },
            );
        }
        self.draw_text(
            ctx.renderer,
            "CLICK A DIFFICULTY   OR  UP/DOWN + SPACE/ENTER",
            center + Vec2::new(-320.0, -180.0),
            2.0,
            Color::rgb(0.6, 0.7, 0.78),
        );
    }

    fn friendly_count(&self, faction: FactionId) -> usize {
        self.world
            .units()
            .iter()
            .filter(|unit| unit.faction == faction && unit.alive())
            .count()
    }

    fn closest_enemy_at(&self, point: Vec2, faction: FactionId) -> Option<aurora_engine::UnitId> {
        self.world
            .units()
            .iter()
            .filter(|unit| unit.faction != faction && unit.alive())
            .filter_map(|unit| {
                let distance = unit.position.distance(point);
                (distance <= unit.radius * 1.8).then_some((unit.id, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    fn update_combat(&mut self, dt: f32) {
        let snapshot: Vec<(aurora_engine::UnitId, Vec2, bool)> = self
            .world
            .units()
            .iter()
            .map(|unit| (unit.id, unit.position, unit.alive()))
            .collect();
        let mut damage = Vec::new();
        for unit in self.world.units() {
            let UnitOrder::Attack(target) = unit.order else {
                continue;
            };
            let Some((_, target_position, true)) =
                snapshot.iter().find(|(id, _, _)| *id == target).copied()
            else {
                continue;
            };
            if unit.position.distance(target_position) <= ATTACK_RANGE {
                damage.push((target, ATTACK_DPS * dt));
            }
        }
        for (target, amount) in damage {
            if let Some(unit) = self.world.unit_mut(target) {
                unit.health = (unit.health - amount).max(0.0);
            }
        }
    }

    fn update_economy(&mut self, dt: f32) {
        self.resource_tick += dt.max(0.0) * 6.0;
        let income = self.resource_tick.floor() as u32;
        if income > 0 {
            self.resources.credit(income);
            self.resource_tick -= income as f32;
        }
        if self.power.is_powered(PLAYER_CORE) {
            for product in self.production.update(dt) {
                if product == TROOPER {
                    let offset = Vec2::new(
                        (self.friendly_count(PLAYER) % 4) as f32 * 40.0 - 60.0,
                        -50.0,
                    );
                    self.world.spawn(PLAYER, PLAYER_BASE + offset);
                    self.status = Some(("TROOPER DEPLOYED".to_owned(), 2.5));
                }
            }
        }
        if let Some((_, remaining)) = self.status.as_mut() {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.status = None;
            }
        }
    }

    fn update_enemy_spawns(&mut self, dt: f32) {
        self.enemy_spawn_timer -= dt;
        if self.enemy_spawn_timer > 0.0 {
            return;
        }
        self.enemy_spawn_timer = self.difficulty.spawn_interval();
        let offset = Vec2::new((self.friendly_count(ENEMY) % 4) as f32 * 40.0 - 60.0, 50.0);
        self.world.spawn(ENEMY, ENEMY_BASE + offset);
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
                    self.world
                        .select_point(mouse_world, PLAYER, ctx.input.shift_down());
                } else {
                    self.world
                        .select_bounds(drag.bounds(), PLAYER, ctx.input.shift_down());
                }
            }
        }
        if ctx.input.mouse_pressed(MouseButton::Right) && !self.world.selection().ids().is_empty() {
            if let Some(enemy) = self.closest_enemy_at(mouse_world, PLAYER) {
                self.world.issue_attack(enemy);
            } else {
                self.world.issue_move(mouse_world, 56.0);
            }
            self.order_marker = Some((mouse_world, 0.6));
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
        ctx.renderer.camera.position += pan * (520.0 / ctx.renderer.camera.zoom) * dt;
        if ctx.input.scroll.abs() > f32::EPSILON {
            ctx.renderer
                .camera
                .zoom_at(1.0 + ctx.input.scroll * 0.09, ctx.input.mouse_position);
        }
        ctx.renderer
            .camera
            .clamp_to_bounds(Aabb::from_center_size(Vec2::ZERO, MAP_SIZE));
    }

    fn draw_text(
        &self,
        renderer: &mut Renderer,
        text: &str,
        origin: Vec2,
        pixel: f32,
        color: Color,
    ) {
        for glyph in aurora_engine::BitmapText::glyphs(text, origin, pixel) {
            renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(glyph.position, Vec2::splat(glyph.size))
                    .with_color(color)
                    .with_z(9.0),
            );
        }
    }
}

impl Game for Skirmish {
    fn name(&self) -> &str {
        "Aurora Engine — Skirmish"
    }

    fn on_start(&mut self, renderer: &mut Renderer) {
        let (player, enemy, base, ui) = {
            let gpu = renderer.gpu();
            (
                Texture::solid(&gpu, Color::rgb(0.22, 0.85, 0.95)),
                Texture::solid(&gpu, Color::rgb(0.95, 0.28, 0.42)),
                Texture::soft_circle(&gpu, 64, Color::WHITE),
                Texture::solid(&gpu, Color::WHITE),
            )
        };
        self.tex_player = renderer.add_texture(player);
        self.tex_enemy = renderer.add_texture(enemy);
        self.tex_base = renderer.add_texture(base);
        self.tex_ui = renderer.add_texture(ui);
        renderer.camera.position = Vec2::ZERO;
        renderer.camera.zoom = 0.85;
        renderer.camera.zoom_min = 0.6;
        renderer.camera.zoom_max = 1.5;
        renderer.set_clear_color(Color::rgb(0.01, 0.015, 0.03));
    }

    fn on_fixed_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let dt = ctx.time.fixed_dt;
        self.update_camera(ctx, dt);
        if self.mode_select {
            self.handle_mode_select(ctx);
            return;
        }
        if self.victory || self.defeat {
            return;
        }

        if ctx.input.key_pressed(KeyCode::KeyQ) {
            let recipe = ProductionRecipe::new(TROOPER, 40, 2_000);
            match self.production.enqueue(recipe, &mut self.resources) {
                Ok(()) => self.status = Some(("TROOPER QUEUED".to_owned(), 2.0)),
                Err(QueueError::InsufficientResources) => {
                    self.status = Some(("INSUFFICIENT SALVAGE".to_owned(), 2.0));
                }
                Err(QueueError::Full) => self.status = Some(("QUEUE FULL".to_owned(), 2.0)),
            }
        }

        self.handle_pointer(ctx);
        self.enemy_think -= dt;
        if self.enemy_think <= 0.0 {
            self.enemy_think = 0.6;
            self.enemy_ai.think(
                &mut self.world,
                ENEMY,
                PLAYER,
                self.match_time,
                &self.difficulty.ai_params(),
                None,
            );
        }
        self.world.update(dt);
        self.update_combat(dt);
        self.update_economy(dt);
        self.update_enemy_spawns(dt);
        self.match_time += dt;
        if let Some((_, time)) = self.order_marker.as_mut() {
            *time -= dt;
            if *time <= 0.0 {
                self.order_marker = None;
            }
        }

        self.defeat = self.friendly_count(PLAYER) == 0;
        self.victory = self.friendly_count(ENEMY) == 0 && !self.defeat;
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        if self.mode_select {
            self.draw_mode_select(ctx);
            return;
        }
        ctx.renderer.draw_sprite(
            self.tex_ui,
            Sprite::new(Vec2::ZERO, MAP_SIZE)
                .with_color(Color::rgb(0.03, 0.035, 0.05))
                .with_z(-10.0),
        );

        let mut base = Sprite::new(PLAYER_BASE, Vec2::splat(220.0)).with_z(-1.0);
        base.color = Color::rgba(0.22, 0.85, 0.95, 0.35);
        ctx.renderer.draw_sprite(self.tex_base, base);
        ctx.renderer.draw_light(PointLight::new(
            PLAYER_BASE,
            Color::rgb(0.22, 0.85, 0.95),
            220.0,
            0.2,
        ));
        let mut base = Sprite::new(ENEMY_BASE, Vec2::splat(220.0)).with_z(-1.0);
        base.color = Color::rgba(0.95, 0.28, 0.42, 0.35);
        ctx.renderer.draw_sprite(self.tex_base, base);
        ctx.renderer.draw_light(PointLight::new(
            ENEMY_BASE,
            Color::rgb(0.95, 0.28, 0.42),
            220.0,
            0.2,
        ));

        for unit in self.world.units() {
            if !unit.alive() {
                continue;
            }
            let texture = if unit.faction == PLAYER {
                self.tex_player
            } else {
                self.tex_enemy
            };
            let selected = self.world.selection().contains(unit.id);
            let size = if selected {
                UNIT_RADIUS * 2.3
            } else {
                UNIT_RADIUS * 2.0
            };
            ctx.renderer.draw_sprite(
                texture,
                Sprite::new(unit.position, Vec2::splat(size)).with_z(0.0),
            );
            let health_fraction = (unit.health / unit.max_health.max(1.0)).clamp(0.0, 1.0);
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(
                    unit.position + Vec2::new(0.0, UNIT_RADIUS + 10.0),
                    Vec2::new(36.0 * health_fraction, 5.0),
                )
                .with_color(Color::rgb(0.3, 1.0, 0.5))
                .with_z(0.2),
            );
        }

        if let Some(drag) = self.drag {
            let bounds = drag.bounds();
            let center = (bounds.min + bounds.max) * 0.5;
            let size = bounds.max - bounds.min;
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(center, size)
                    .with_color(Color::rgba(0.3, 1.0, 0.8, 0.12))
                    .with_z(5.0),
            );
        }
        if let Some((position, _)) = self.order_marker {
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(position, Vec2::splat(26.0))
                    .with_color(Color::rgba(1.0, 0.8, 0.3, 0.8))
                    .with_z(5.0),
            );
        }

        let top_left = ctx
            .renderer
            .camera
            .world_from_viewport_fraction(Vec2::new(0.0, 1.0))
            + Vec2::new(30.0, -34.0);
        self.draw_text(
            ctx.renderer,
            &format!(
                "SALVAGE {}   TROOPERS {}   ENEMY {}",
                self.resources.amount(),
                self.friendly_count(PLAYER),
                self.friendly_count(ENEMY)
            ),
            top_left,
            3.6,
            Color::rgb(0.73, 1.15, 1.08),
        );
        if let Some((message, _)) = &self.status {
            self.draw_text(
                ctx.renderer,
                message,
                top_left + Vec2::new(0.0, -30.0),
                2.4,
                Color::rgb(0.65, 1.15, 1.05),
            );
        }

        let queue_label = self
            .production
            .items()
            .front()
            .map(|item| {
                format!(
                    "BUILDING TROOPER  {:02}%  QUEUE {}",
                    (item.progress() * 100.0) as u32,
                    self.production.items().len()
                )
            })
            .unwrap_or_else(|| "QUEUE READY".to_owned());
        self.draw_text(
            ctx.renderer,
            &queue_label,
            top_left + Vec2::new(0.0, -58.0),
            2.0,
            Color::rgb(1.15, 0.7, 0.25),
        );
        if let Some(progress) = self.production.items().front().map(|item| item.progress()) {
            let bar_origin = top_left + Vec2::new(0.0, -78.0);
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

        if self.victory || self.defeat {
            let center = ctx.renderer.camera.position;
            ctx.renderer.draw_sprite(
                self.tex_ui,
                Sprite::new(center, Vec2::new(560.0, 220.0))
                    .with_color(Color::rgba(0.01, 0.02, 0.045, 0.9))
                    .with_z(10.0),
            );
            let (title, color) = if self.victory {
                ("ENEMY GARRISON DESTROYED", Color::rgb(0.3, 1.5, 1.0))
            } else {
                ("BASE OVERRUN", Color::rgb(1.4, 0.4, 0.35))
            };
            self.draw_text(
                ctx.renderer,
                title,
                center + Vec2::new(-260.0, 20.0),
                4.5,
                color,
            );
            self.draw_text(
                ctx.renderer,
                "RESTART THE GAME TO PLAY AGAIN",
                center + Vec2::new(-260.0, -30.0),
                2.4,
                Color::rgb(0.75, 0.8, 0.85),
            );
        }
    }
}

fn main() {
    run(Skirmish::default());
}
