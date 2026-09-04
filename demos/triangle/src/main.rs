use aurora_engine::{
    Aabb, Color, FlubberId, FrameCtx, Game, RtsWorld, Sprite, Texture, TextureHandle,
};
use glam::Vec2;
use winit::{event::MouseButton, keyboard::KeyCode};

#[derive(Debug, Clone, Copy)]
enum DemoMode {
    Flubber,
    Build,
}

impl DemoMode {
    fn label(self) -> &'static str {
        match self {
            Self::Flubber => "FLUBBER",
            Self::Build => "BUILD",
        }
    }

    fn toggle(self) -> Self {
        match self {
            Self::Flubber => Self::Build,
            Self::Build => Self::Flubber,
        }
    }
}

/// Engine-only sandbox for the flubber prototype.
pub struct FlubberDemo {
    pub title: String,
    world: RtsWorld,
    mode: DemoMode,
    selected_flubber: Option<FlubberId>,
    tex_flubber: Option<TextureHandle>,
    tex_core: Option<TextureHandle>,
    tex_block: Option<TextureHandle>,
    tex_blob: Option<TextureHandle>,
    tex_ui: Option<TextureHandle>,
}

impl Default for FlubberDemo {
    fn default() -> Self {
        Self {
            title: "Aurora Engine — Flubber".into(),
            world: RtsWorld::default(),
            mode: DemoMode::Flubber,
            selected_flubber: None,
            tex_flubber: None,
            tex_core: None,
            tex_block: None,
            tex_blob: None,
            tex_ui: None,
        }
    }
}

impl FlubberDemo {
    fn draw_text(
        &self,
        renderer: &mut aurora_engine::Renderer,
        text: &str,
        origin: Vec2,
        pixel: f32,
        color: Color,
    ) {
        let Some(ui) = self.tex_ui else {
            return;
        };
        for glyph in aurora_engine::BitmapText::glyphs(text, origin, pixel) {
            renderer.draw_sprite(
                ui,
                Sprite::new(glyph.position, Vec2::splat(glyph.size))
                    .with_color(color)
                    .with_z(9.0),
            );
        }
    }

    fn tune_selected_flubber(&mut self, ctx: &mut FrameCtx<'_>, id: FlubberId) {
        let Some(flubber) = self.world.flubber_mut(id) else {
            self.selected_flubber = None;
            return;
        };

        if ctx.input.key_pressed(KeyCode::Minus) {
            flubber.elasticity = (flubber.elasticity - 1.2).max(0.0);
        }
        if ctx.input.key_pressed(KeyCode::Equal) {
            flubber.elasticity += 1.2;
        }

        if ctx.input.key_pressed(KeyCode::BracketLeft) {
            flubber.damping = (flubber.damping - 1.0).max(0.0);
        }
        if ctx.input.key_pressed(KeyCode::BracketRight) {
            flubber.damping += 1.0;
        }

        if ctx.input.key_pressed(KeyCode::Comma) {
            flubber.restitution = (flubber.restitution - 0.04).max(0.0);
        }
        if ctx.input.key_pressed(KeyCode::Period) {
            flubber.restitution = (flubber.restitution + 0.04).min(1.0);
        }

        if ctx.input.key_pressed(KeyCode::Semicolon) {
            flubber.speed_cap = (flubber.speed_cap - 340.0).max(80.0);
        }
        if ctx.input.key_pressed(KeyCode::Quote) {
            flubber.speed_cap += 340.0;
        }

        if ctx.input.key_pressed(KeyCode::KeyQ) {
            flubber.split_threshold = (flubber.split_threshold - 280.0).max(500.0);
        }
        if ctx.input.key_pressed(KeyCode::KeyW) {
            flubber.split_threshold += 280.0;
        }
    }
}

impl Game for FlubberDemo {
    fn name(&self) -> &str {
        &self.title
    }

    fn on_start(&mut self, renderer: &mut aurora_engine::Renderer) {
        renderer.set_clear_color(Color::rgb(0.025, 0.03, 0.05));
        let flubber_texture = {
            let gpu = renderer.gpu();
            Texture::soft_circle(&gpu, 128, Color::rgba(0.72, 0.96, 1.0, 0.95))
        };
        let core_texture = {
            let gpu = renderer.gpu();
            Texture::solid(&gpu, Color::rgba(0.95, 0.98, 1.0, 0.88))
        };
        let block_texture = {
            let gpu = renderer.gpu();
            Texture::checker(
                &gpu,
                128,
                8,
                Color::rgb(0.1, 0.18, 0.34),
                Color::rgba(0.16, 0.26, 0.48, 0.92),
            )
        };
        let blob_texture = {
            let gpu = renderer.gpu();
            Texture::soft_circle(&gpu, 96, Color::rgba(0.98, 0.88, 1.0, 0.82))
        };
        let ui_texture = {
            let gpu = renderer.gpu();
            Texture::solid(&gpu, Color::WHITE)
        };

        self.tex_flubber = Some(renderer.add_texture(flubber_texture));
        self.tex_core = Some(renderer.add_texture(core_texture));
        self.tex_block = Some(renderer.add_texture(block_texture));
        self.tex_blob = Some(renderer.add_texture(blob_texture));
        self.tex_ui = Some(renderer.add_texture(ui_texture));

        self.world = RtsWorld::default();
        self.world.add_block_obstacle(Aabb::from_center_size(
            Vec2::new(-250.0, 0.0),
            Vec2::new(90.0, 260.0),
        ));
        self.world.add_block_obstacle(Aabb::from_center_size(
            Vec2::new(220.0, -60.0),
            Vec2::new(140.0, 170.0),
        ));
        self.world.add_block_obstacle(Aabb::from_center_size(
            Vec2::new(0.0, 250.0),
            Vec2::new(260.0, 80.0),
        ));

        self.world
            .add_blob_obstacle(Vec2::new(-180.0, -120.0), 38.0);
        self.world.add_blob_obstacle(Vec2::new(190.0, 150.0), 34.0);

        let _ = self.world.add_flubber(Vec2::new(40.0, 40.0), 56.0);
        let _ = self.world.add_flubber(Vec2::new(-140.0, -150.0), 34.0);
        self.selected_flubber = self.world.flubbers().first().map(|flubber| flubber.id);
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        if let Some(selected_flubber) = self.selected_flubber {
            if self.world.flubber(selected_flubber).is_none() {
                self.selected_flubber = None;
            }
        }

        if self.selected_flubber.is_none() {
            self.selected_flubber = self.world.flubbers().first().map(|flubber| flubber.id);
        }

        if ctx.input.key_pressed(KeyCode::KeyB) {
            self.mode = self.mode.toggle();
        }
        if ctx.input.key_pressed(KeyCode::KeyC) {
            self.world.clear_motion_obstacles();
        }

        let build_scale = (1.0 + ctx.input.scroll * 0.20).clamp(0.5, 3.0);
        let boop_scale = (1.0 + ctx.input.scroll * 0.40).clamp(0.15, 5.0);
        let mouse_world = ctx
            .renderer
            .camera
            .screen_to_world(ctx.input.mouse_position);

        let shift_down = ctx.input.shift_down();
        if ctx.input.mouse_pressed(MouseButton::Middle) && shift_down {
            let removed_blob = self
                .world
                .remove_blob_obstacle_at(mouse_world, 0.65)
                .is_some();
            if !removed_blob {
                let _ = self.world.remove_block_obstacle_at(mouse_world);
            }
        } else if ctx.input.mouse_pressed(MouseButton::Middle) {
            let radius = (36.0 * build_scale).max(14.0);
            if let Some(id) = self.world.add_flubber(mouse_world, radius) {
                self.selected_flubber = Some(id);
            }
        }

        if ctx.input.mouse_pressed(MouseButton::Left) {
            match self.mode {
                DemoMode::Flubber => {
                    if let Some(id) =
                        self.world
                            .slap_flubber_at(mouse_world, 210.0, 1700.0 * (1.0 + boop_scale))
                    {
                        self.selected_flubber = Some(id);
                    }
                }
                DemoMode::Build => {
                    if shift_down {
                        let _ = self.world.remove_block_obstacle_at(mouse_world);
                    } else {
                        let size = Vec2::new(90.0, 230.0) * build_scale;
                        self.world
                            .add_block_obstacle(Aabb::from_center_size(mouse_world, size));
                    }
                }
            }
        }

        if ctx.input.mouse_pressed(MouseButton::Right) {
            match self.mode {
                DemoMode::Flubber => {
                    if let Some(id) =
                        self.world
                            .slap_flubber_at(mouse_world, 250.0, 3900.0 * boop_scale)
                    {
                        self.selected_flubber = Some(id);
                    }
                }
                DemoMode::Build => {
                    self.world
                        .add_blob_obstacle(mouse_world, (28.0 * build_scale).max(12.0));
                }
            }
        }

        if let Some(selected_flubber) = self.selected_flubber {
            if ctx.input.key_pressed(KeyCode::KeyE) {
                self.energy_pulse(selected_flubber, mouse_world, 1.25);
            }
        }

        if let Some(selected_flubber) = self.selected_flubber {
            self.tune_selected_flubber(ctx, selected_flubber);
        }

        self.world.update(ctx.time.delta);

        if let (Some(flubber_tex), Some(core_tex), Some(block_tex), Some(blob_tex), Some(_ui_tex)) = (
            self.tex_flubber,
            self.tex_core,
            self.tex_block,
            self.tex_blob,
            self.tex_ui,
        ) {
            for block in self.world.block_obstacles() {
                let size = block.bounds.size();
                ctx.renderer.draw_sprite(
                    block_tex,
                    Sprite::new(block.bounds.center(), size * 1.02)
                        .with_color(Color::rgb(0.16, 0.24, 0.46))
                        .with_z(0.1),
                );
            }

            for blob in self.world.blob_obstacles() {
                ctx.renderer.draw_sprite(
                    blob_tex,
                    Sprite::new(blob.center, Vec2::splat(blob.radius * 2.0 * 2.2))
                        .with_color(Color::rgb(0.56, 0.62, 0.84))
                        .with_z(1.0),
                );
            }

            for flubber in self.world.flubbers() {
                let stretch_ratio = flubber.stretch.length().min(flubber.max_stretch)
                    / flubber.max_stretch.max(1.0);
                let radius =
                    (flubber.radius + flubber.stretch.length().min(flubber.max_stretch)) * 2.0;
                let outer_size = Vec2::splat(radius * 1.12);
                let core_size = Vec2::splat((radius * 0.35).max(16.0));
                let shade = 0.28 + stretch_ratio * 0.72;
                let is_selected = self.selected_flubber.is_some_and(|id| id == flubber.id);
                let tint = if is_selected {
                    Color::rgb(1.0, 1.0, 0.6)
                } else {
                    Color::rgb(0.75, 0.75, 1.0)
                };
                ctx.renderer.draw_sprite(
                    flubber_tex,
                    Sprite::new(flubber.position, outer_size)
                        .with_rotation(stretch_ratio * 0.6)
                        .with_color(Color::rgba(
                            shade,
                            0.75,
                            1.0 - stretch_ratio * 0.3,
                            if is_selected { 1.0 } else { 0.95 },
                        ))
                        .with_z(2.0),
                );
                ctx.renderer.draw_sprite(
                    core_tex,
                    Sprite::new(flubber.position, core_size)
                        .with_rotation(-stretch_ratio * 0.45)
                        .with_color(tint),
                );
            }

            self.draw_text(
                ctx.renderer,
                "AURORA ENGINE — FLUBBER LAB",
                Vec2::new(-700.0, 360.0),
                4.4,
                Color::rgb(0.16, 1.0, 1.0),
            );
            let mode_line = format!(
                "MODE: {}  (B: toggle | C: clear obstacles)",
                self.mode.label()
            );
            let scale_line = format!("SCALE: {build_scale:.2} | BOOP: {boop_scale:.2}");
            let selected_line = if let Some(flubber) =
                self.selected_flubber.and_then(|id| self.world.flubber(id))
            {
                format!(
                    "SELECTED F#{:03}  |  E:{:.1}  D:{:.1}  R:{:.2}  SPD:{:.0}  SPLIT:{:.0}",
                    flubber.id.0,
                    flubber.elasticity,
                    flubber.damping,
                    flubber.restitution,
                    flubber.speed_cap,
                    flubber.split_threshold,
                )
            } else {
                "SELECTED F: none".to_string()
            };
            let controls = "LMB slap/spawn block | RMB boop/spawn blob | MMB spawn flubber | SHIFT+MMB remove obstacle | SHIFT+LMB remove block in build mode | KEYE energy pulse";
            let tuning = "[/-] elast  [ / ] damping  ,/. restitution  ;/' speedcap  Q/W split";

            self.draw_text(
                ctx.renderer,
                &mode_line,
                Vec2::new(-700.0, 332.0),
                2.9,
                Color::rgb(0.9, 0.99, 1.0),
            );
            self.draw_text(
                ctx.renderer,
                &scale_line,
                Vec2::new(-700.0, 309.0),
                2.4,
                Color::rgb(0.85, 0.92, 1.0),
            );
            self.draw_text(
                ctx.renderer,
                &selected_line,
                Vec2::new(-700.0, 286.0),
                2.4,
                Color::rgb(0.85, 0.92, 1.0),
            );
            self.draw_text(
                ctx.renderer,
                controls,
                Vec2::new(-700.0, 263.0),
                2.0,
                Color::rgb(0.6, 0.78, 1.0),
            );
            self.draw_text(
                ctx.renderer,
                tuning,
                Vec2::new(-700.0, 241.0),
                2.0,
                Color::rgb(0.6, 0.78, 1.0),
            );
        }
    }
}

fn main() {
    aurora_engine::run(FlubberDemo::default());
}

impl FlubberDemo {
    fn energy_pulse(&mut self, source_id: FlubberId, cursor: Vec2, intensity: f32) {
        let Some(source) = self.world.flubber(source_id) else {
            self.selected_flubber = None;
            return;
        };

        let source_mass = source.mass;
        let source_velocity = source.velocity;
        let source_position = source.position;
        let radius = 260.0;
        let impulse_scale = intensity * 0.15 * source_mass.max(0.001);
        let mut pulse = Vec2::ZERO;

        let targets: Vec<(FlubberId, Vec2)> = self
            .world
            .flubbers()
            .iter()
            .filter_map(|target| {
                if target.id == source_id {
                    return None;
                }
                let to_target = target.position - source_position;
                let distance = to_target.length();
                if distance <= f32::EPSILON || distance > radius {
                    return None;
                }
                if !target.position.is_finite() || !to_target.is_finite() {
                    return None;
                }
                Some((target.id, target.position))
            })
            .collect();

        for (target_id, target_position) in targets {
            let to_target = target_position - source_position;
            let distance = to_target.length();
            let falloff = 1.0 - (distance / radius).min(1.0);
            let direction = if distance <= f32::EPSILON {
                cursor - source_position
            } else {
                to_target
            };
            let dir = if direction.length_squared() <= f32::EPSILON {
                Vec2::Y
            } else {
                direction / direction.length()
            };
            let base_impulse = if source_velocity.length_squared() > f32::EPSILON {
                source_velocity.normalize() * source_velocity.length() * falloff * impulse_scale
            } else {
                dir * 1200.0 * falloff * impulse_scale
            };
            let _ = self.world.slap_flubber(target_id, base_impulse);
            pulse += -base_impulse;
        }

        if pulse.length() > f32::EPSILON {
            let recoil = pulse * 0.25 / source_mass.max(1.0);
            let _ = self.world.slap_flubber(source_id, recoil);
        }
    }
}
