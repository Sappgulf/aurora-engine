//! Milestone 4 core pipeline smoke test: an orbiting camera around a lit,
//! depth-tested 3D scene (cube + sphere, single directional light, PBR).
//!
//! Also proves the glTF loader (a test-cube GLB loaded at startup, from disk
//! on native / embedded on wasm), the directional-light shadow map, and the
//! gradient sky. Keyboard toggles: [S] shadows, [K] sky.

use aurora_engine::gltf::GltfScene;
use aurora_engine::{
    run, Camera3D, Color, FrameCtx, Game, Material3D, Mesh3D, Mesh3DHandle, Renderer,
};
use glam::{Mat4, Vec3};
use winit::keyboard::KeyCode;

#[cfg(any(test, not(target_arch = "wasm32")))]
const ASSET_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/test-cube.glb");

struct MeshDemo {
    camera: Camera3D,
    ground: Option<Mesh3DHandle>,
    cube: Option<Mesh3DHandle>,
    sphere: Option<Mesh3DHandle>,
    gltf_parts: Vec<(Mesh3DHandle, Material3D)>,
}

impl Default for MeshDemo {
    fn default() -> Self {
        Self {
            camera: Camera3D::new(1280.0, 720.0),
            ground: None,
            cube: None,
            sphere: None,
            gltf_parts: Vec::new(),
        }
    }
}

impl Game for MeshDemo {
    fn name(&self) -> &str {
        "Aurora Engine — Mesh Demo"
    }

    fn on_start(&mut self, renderer: &mut Renderer) {
        self.ground = Some(renderer.upload_mesh3d(&Mesh3D::unit_plane()));
        self.cube = Some(renderer.upload_mesh3d(&Mesh3D::cube()));
        self.sphere = Some(renderer.upload_mesh3d(&Mesh3D::uv_sphere(32, 16)));
        let bytes = load_test_cube_bytes();
        match GltfScene::from_bytes(&bytes) {
            Ok(scene) => {
                self.gltf_parts = scene
                    .meshes
                    .into_iter()
                    .map(|part| (renderer.upload_mesh3d(&part.mesh), part.material))
                    .collect();
            }
            Err(error) => log::warn!("failed to load test-cube.glb: {error}"),
        }
        renderer.set_directional_light(Vec3::new(-0.4, -1.0, -0.3), Color::WHITE, 3.0);
        renderer.set_clear_color(Color::AURORA_NIGHT);
        log::info!("Controls: [S] toggle shadows, [K] toggle sky, [H] hot-reload shaders");
    }

    fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
        let size = ctx.renderer.size();
        self.camera
            .set_viewport(size.width as f32, size.height as f32);

        let t = ctx.time.elapsed;
        let angle = t * 0.4;
        self.camera.position = Vec3::new(angle.sin() * 4.0, 1.8, angle.cos() * 4.0);
        self.camera.target = Vec3::new(0.0, -0.1, 0.0);
        ctx.renderer.set_camera3d(&self.camera);

        if ctx.input.key_pressed(KeyCode::KeyS) {
            let mut shadows = ctx.renderer.shadow_settings();
            shadows.enabled = !shadows.enabled;
            ctx.renderer.set_shadow_settings(shadows);
            log::info!("shadows: {}", if shadows.enabled { "on" } else { "off" });
        }
        if ctx.input.key_pressed(KeyCode::KeyK) {
            let mut sky = ctx.renderer.sky_settings();
            sky.enabled = !sky.enabled;
            ctx.renderer.set_sky_settings(sky);
            log::info!("sky: {}", if sky.enabled { "on" } else { "off" });
        }
        #[cfg(not(target_arch = "wasm32"))]
        if ctx.input.key_pressed(KeyCode::KeyH) {
            if ctx.renderer.shader_dir().is_none() {
                ctx.renderer.set_shader_dir(Some(std::path::PathBuf::from(
                    "crates/aurora-engine/shaders",
                )));
                log::info!("shader overrides: crates/aurora-engine/shaders — edit a .wgsl then press H again");
            }
            match ctx.renderer.reload_shaders() {
                Ok(()) => log::info!("shaders reloaded from disk"),
                Err(errors) => log::warn!("shader reload kept previous pipelines: {errors:?}"),
            }
        }

        if let Some(ground) = self.ground {
            let ground_transform = Mat4::from_translation(Vec3::new(0.0, -0.75, 0.0))
                * Mat4::from_scale(Vec3::new(12.0, 1.0, 12.0));
            ctx.renderer.queue_mesh3d(
                ground,
                ground_transform,
                Material3D {
                    base_color: Color::rgb(0.75, 0.78, 0.82),
                    metallic: 0.0,
                    roughness: 0.9,
                    emissive: Color::BLACK,
                },
            );
        }

        if let Some(cube) = self.cube {
            let transform =
                Mat4::from_translation(Vec3::new(-1.0, 0.4, 0.0)) * Mat4::from_rotation_y(t * 0.6);
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
            let transform = Mat4::from_translation(Vec3::new(1.0, 0.3, 0.0));
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

        let gltf_transform = Mat4::from_translation(Vec3::new(0.0, -0.05, 0.0))
            * Mat4::from_rotation_y(-t * 0.35)
            * Mat4::from_scale(Vec3::splat(1.4));
        for (handle, material) in &self.gltf_parts {
            ctx.renderer
                .queue_mesh3d(*handle, gltf_transform, *material);
        }
    }
}

fn main() {
    run(MeshDemo::default());
}

#[cfg(not(target_arch = "wasm32"))]
fn load_test_cube_bytes() -> Vec<u8> {
    std::fs::read(ASSET_PATH).unwrap_or_else(|error| panic!("failed to read {ASSET_PATH}: {error}"))
}

#[cfg(target_arch = "wasm32")]
fn load_test_cube_bytes() -> Vec<u8> {
    include_bytes!("../assets/test-cube.glb").to_vec()
}

/// Serializes Aurora's test cube into a minimal binary glTF 2.0 container
/// (positions, normals, UVs, indices, one PBR material).
#[cfg(test)]
fn test_cube_glb() -> Vec<u8> {
    let cube = Mesh3D::cube();
    let mut positions = Vec::with_capacity(cube.vertices.len() * 3);
    let mut normals = Vec::with_capacity(cube.vertices.len() * 3);
    let mut uvs = Vec::with_capacity(cube.vertices.len() * 2);
    for vertex in &cube.vertices {
        positions.extend_from_slice(&vertex.position.to_array());
        normals.extend_from_slice(&vertex.normal.to_array());
        uvs.extend_from_slice(&vertex.uv.to_array());
    }

    let position_bytes = f32_bytes(&positions);
    let normal_bytes = f32_bytes(&normals);
    let uv_bytes = f32_bytes(&uvs);
    let index_bytes = u32_bytes(&cube.indices);
    let position_len = position_bytes.len();
    let normal_offset = position_len;
    let normal_len = normal_bytes.len();
    let uv_offset = normal_offset + normal_len;
    let uv_len = uv_bytes.len();
    let index_offset = uv_offset + uv_len;
    let index_len = index_bytes.len();
    let mut bin = position_bytes;
    bin.extend(normal_bytes);
    bin.extend(uv_bytes);
    bin.extend(index_bytes);
    let bin_len = bin.len();

    let vertex_count = cube.vertices.len() as u32;
    let index_count = cube.indices.len() as u32;
    let json = format!(
        r#"{{
  "asset": {{"version": "2.0"}},
  "scene": 0,
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0}}],
  "buffers": [{{"byteLength": {bin_len}}}],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": {position_len}}},
    {{"buffer": 0, "byteOffset": {normal_offset}, "byteLength": {normal_len}}},
    {{"buffer": 0, "byteOffset": {uv_offset}, "byteLength": {uv_len}}},
    {{"buffer": 0, "byteOffset": {index_offset}, "byteLength": {index_len}}}
  ],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": {vertex_count}, "type": "VEC3",
      "min": [-0.5, -0.5, -0.5], "max": [0.5, 0.5, 0.5]}},
    {{"bufferView": 1, "componentType": 5126, "count": {vertex_count}, "type": "VEC3"}},
    {{"bufferView": 2, "componentType": 5126, "count": {vertex_count}, "type": "VEC2"}},
    {{"bufferView": 3, "componentType": 5125, "count": {index_count}, "type": "SCALAR"}}
  ],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0, "NORMAL": 1,
    "TEXCOORD_0": 2}}, "indices": 3, "material": 0}}]}}],
  "materials": [{{"pbrMetallicRoughness": {{
    "baseColorFactor": [0.95, 0.76, 0.28, 1.0], "metallicFactor": 0.25,
    "roughnessFactor": 0.45}}, "emissiveFactor": [0.0, 0.0, 0.0]}}]
}}"#
    );
    pack_glb(&json, &bin)
}

#[cfg(test)]
fn f32_bytes(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[cfg(test)]
fn u32_bytes(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[cfg(test)]
fn pack_glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json_bytes = json.as_bytes().to_vec();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let mut bin_bytes = bin.to_vec();
    while !bin_bytes.len().is_multiple_of(4) {
        bin_bytes.push(0);
    }
    let total = (12 + 8 + json_bytes.len() + 8 + bin_bytes.len()) as u32;
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(&0x4654_6C67u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
    out.extend_from_slice(&bin_bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regenerates `assets/test-cube.glb` from code. Run explicitly with:
    /// `cargo test -p mesh_demo --features 3d -- --ignored export_test_cube_asset`
    #[test]
    #[ignore]
    fn export_test_cube_asset() {
        std::fs::write(ASSET_PATH, test_cube_glb()).expect("write test-cube.glb");
    }

    #[test]
    fn committed_test_cube_asset_is_current() {
        let committed = std::fs::read(ASSET_PATH).unwrap_or_else(|error| {
            panic!(
                "missing {ASSET_PATH} ({error}); run: cargo test -p mesh_demo --features 3d -- \
                 --ignored export_test_cube_asset"
            )
        });
        assert_eq!(committed, test_cube_glb());
    }

    #[test]
    fn committed_test_cube_asset_parses() {
        let scene = GltfScene::from_bytes(&test_cube_glb()).expect("committed test cube parses");
        assert_eq!(scene.meshes.len(), 1);
        assert_eq!(scene.meshes[0].mesh.vertices.len(), 24);
        assert_eq!(scene.meshes[0].mesh.indices.len(), 36);
        assert_eq!(scene.meshes[0].material.metallic, 0.25);
    }
}
