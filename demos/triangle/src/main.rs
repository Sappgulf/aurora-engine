use aurora_engine::{
    Aabb, Color, FlubberId, FrameCtx, Game, RtsWorld, Sprite, Texture, TextureHandle,
};
use glam::Vec2;
use winit::event::MouseButton;

#[derive(Debug, Clone, Copy)]
struct SceneRect {
    center: Vec2,
    size: Vec2,
    color: Color,
}

#[derive(Debug, Clone, Copy)]
struct SceneBlob {
    center: Vec2,
    radius: f32,
    color: Color,
}

/// Engine-only sandbox for the flubber prototype.
pub struct FlubberDemo {
    pub title: String,
    world: RtsWorld,
    flubber_id: Option<FlubberId>,
    blocks: Vec<SceneRect>,
    blobs: Vec<SceneBlob>,
    tex_flubber: Option<TextureHandle>,
    tex_core: Option<TextureHandle>,
    tex_block: Option<TextureHandle>,
    tex_blob: Option<TextureHandle>,
}

impl Default for FlubberDemo {
    fn default() -> Self {
        let blocks = vec![
            SceneRect {
                center: Vec2::new(-250.0, 0.0),
                size: Vec2::new(90.0, 260.0),
                color: Color::rgb(0.16, 0.24, 0.46),
            },
            SceneRect {
                center: Vec2::new(220.0, -60.0),
                size: Vec2::new(140.0, 170.0),
                color: Color::rgb(0.26, 0.22, 0.52),
            },
            SceneRect {
                center: Vec2::new(0.0, 250.0),
                size: Vec2::new(260.0, 80.0),
                color: Color::rgb(0.15, 0.30, 0.55),
            },
        ];
        let blobs = vec![
            SceneBlob {
                center: Vec2::new(-180.0, -120.0),
                radius: 38.0,
                color: Color::rgb(0.56, 0.62, 0.84),
            },
            SceneBlob {
                center: Vec2::new(190.0, 150.0),
                radius: 34.0,
                color: Color::rgb(0.75, 0.55, 0.82),
            },
        ];
        Self {
            title: "Aurora Engine — Flubber".into(),
            world: RtsWorld::default(),
            flubber_id: None,
            blocks,
            blobs,
            tex_flubber: None,
            tex_core: None,
            tex_block: None,
            tex_blob: None,
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

        self.tex_flubber = Some(renderer.add_texture(flubber_texture));
        self.tex_core = Some(renderer.add_texture(core_texture));
        self.tex_block = Some(renderer.add_texture(block_texture));
        self.tex_blob = Some(renderer.add_texture(blob_texture));

        self.blocks.iter().for_each(|block| {
            let _ = self.world.add_block_obstacle(Aabb::from_center_size(
                block.center,
                block.size,
            ));
        });
        self.blobs.iter().for_each(|blob| {
            let _ = self.world.add_blob_obstacle(blob.center, blob.radius);
        });
        self.flubber_id = self.world.add_flubber(Vec2::new(40.0, 40.0), 56.0);
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        if let Some(flubber_id) = self.flubber_id {
            if self.world.flubber(flubber_id).is_none() {
                self.flubber_id = None;
            }
        }

        if self.flubber_id.is_none() {
            self.flubber_id = self.world.add_flubber(Vec2::ZERO, 56.0);
        }

        if ctx.input.mouse_pressed(MouseButton::Left) {
            let mouse_world = ctx
                .renderer
                .camera
                .screen_to_world(ctx.input.mouse_position);
            let _ = self.world.slap_flubber_at(
                mouse_world,
                180.0,
                3200.0 * (1.0 + (ctx.time.delta * 0.6).min(2.0)),
            );
        }

        self.world.update(ctx.time.delta);

        if let (Some(flubber_tex), Some(core_tex), Some(block_tex), Some(blob_tex)) = (
            self.tex_flubber,
            self.tex_core,
            self.tex_block,
            self.tex_blob,
        ) {
            for obstacle in &self.blocks {
                ctx.renderer.draw_sprite(
                    block_tex,
                    Sprite::new(obstacle.center, obstacle.size).with_color(obstacle.color),
                );
            }
            for blob in &self.blobs {
                ctx.renderer.draw_sprite(
                    blob_tex,
                    Sprite::new(
                        blob.center,
                        Vec2::splat(blob.radius * 2.0 * 2.2),
                    )
                    .with_color(blob.color)
                    .with_z(1.0),
                );
            }

            for flubber in self.world.flubbers() {
                let stretch_ratio = flubber
                    .stretch
                    .length()
                    .min(flubber.max_stretch)
                    / flubber.max_stretch.max(1.0);
                let radius = (flubber.radius + flubber.stretch.length().min(flubber.max_stretch)) * 2.0;
                let outer_size = Vec2::splat(radius * 1.12);
                let core_size = Vec2::splat((radius * 0.35).max(16.0));
                let shade = 0.28 + stretch_ratio * 0.72;
                ctx.renderer.draw_sprite(
                    flubber_tex,
                    Sprite::new(flubber.position, outer_size)
                        .with_rotation(stretch_ratio * 0.6)
                        .with_color(Color::rgba(shade, 0.75, 1.0 - stretch_ratio * 0.3, 0.95)),
                );
                ctx.renderer.draw_sprite(
                    core_tex,
                    Sprite::new(flubber.position, core_size)
                        .with_rotation(-stretch_ratio * 0.45)
                        .with_color(Color::rgba(1.0, 1.0, 1.0, 0.85)),
                );
            }
        }
    }
}

fn main() {
    aurora_engine::run(FlubberDemo::default());
}
