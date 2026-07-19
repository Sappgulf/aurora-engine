//! Milestone 4 core pipeline smoke test: an orbiting camera around a lit,
//! depth-tested 3D scene (cube + sphere, single directional light, PBR).

use aurora_engine::{
    run, Camera3D, Color, FrameCtx, Game, Material3D, Mesh3D, Mesh3DHandle, Renderer,
};
use glam::{Mat4, Vec3};

struct MeshDemo {
    camera: Camera3D,
    cube: Option<Mesh3DHandle>,
    sphere: Option<Mesh3DHandle>,
}

impl Default for MeshDemo {
    fn default() -> Self {
        Self {
            camera: Camera3D::new(1280.0, 720.0),
            cube: None,
            sphere: None,
        }
    }
}

impl Game for MeshDemo {
    fn name(&self) -> &str {
        "Aurora Engine — Mesh Demo"
    }

    fn on_start(&mut self, renderer: &mut Renderer) {
        self.cube = Some(renderer.upload_mesh3d(&Mesh3D::cube()));
        self.sphere = Some(renderer.upload_mesh3d(&Mesh3D::uv_sphere(32, 16)));
        renderer.set_directional_light(Vec3::new(-0.4, -1.0, -0.3), Color::WHITE, 3.0);
        renderer.set_clear_color(Color::AURORA_NIGHT);
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let size = ctx.renderer.size();
        self.camera
            .set_viewport(size.width as f32, size.height as f32);

        let t = ctx.time.elapsed;
        let angle = t * 0.4;
        self.camera.position = Vec3::new(angle.sin() * 4.0, 1.8, angle.cos() * 4.0);
        self.camera.target = Vec3::new(0.0, 0.2, 0.0);
        ctx.renderer.set_camera3d(&self.camera);

        if let Some(cube) = self.cube {
            let transform =
                Mat4::from_translation(Vec3::new(-1.0, 0.0, 0.0)) * Mat4::from_rotation_y(t * 0.6);
            ctx.renderer.queue_mesh3d(
                cube,
                transform,
                Material3D {
                    base_color: Color::rgb(0.85, 0.25, 0.35),
                    metallic: 0.1,
                    roughness: 0.45,
                    emissive: Color::BLACK,
                },
            );
        }

        if let Some(sphere) = self.sphere {
            let transform = Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0));
            ctx.renderer.queue_mesh3d(
                sphere,
                transform,
                Material3D {
                    base_color: Color::rgb(0.3, 0.55, 0.95),
                    metallic: 0.9,
                    roughness: 0.15,
                    emissive: Color::BLACK,
                },
            );
        }
    }
}

fn main() {
    run(MeshDemo::default());
}
