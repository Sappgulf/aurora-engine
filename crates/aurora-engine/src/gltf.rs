//! Minimal glTF 2.0 mesh loading for the 3D renderer.
//!
//! Parsing is bytes-based (`from_bytes`) so the same path works on desktop and
//! wasm: both binary `.glb` containers and `.gltf` JSON with an embedded
//! base64 buffer are accepted. Node transforms are baked into vertex data, so
//! the result is a flat list of static [`Mesh3D`] + [`Material3D`] pairs ready
//! for [`crate::Renderer::upload_mesh3d`].

use std::collections::HashSet;
use std::fmt;

use glam::{Mat3, Mat4, Vec2, Vec3};
use gltf::buffer::Source;

use crate::color::Color;
use crate::mesh3d::{Material3D, Mesh3D, MeshError, MeshVertex};

/// One baked mesh primitive together with its resolved material.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfMeshPart {
    pub mesh: Mesh3D,
    pub material: Material3D,
}

/// A parsed glTF scene reduced to flat, transform-baked mesh parts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GltfScene {
    pub meshes: Vec<GltfMeshPart>,
}

/// Errors produced while loading a glTF scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GltfError {
    /// Input contained no bytes at all.
    EmptyInput,
    /// The JSON body or GLB container could not be parsed.
    Parse(String),
    /// A required buffer, accessor, or attribute was missing.
    MissingData(&'static str),
    /// The file uses a feature outside Aurora's loading subset.
    Unsupported(&'static str),
    /// Parsed geometry violated [`Mesh3D`]'s invariants.
    InvalidGeometry(MeshError),
}

impl fmt::Display for GltfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "glTF input was empty"),
            Self::Parse(error) => write!(f, "glTF parse failed: {error}"),
            Self::MissingData(what) => write!(f, "glTF is missing {what}"),
            Self::Unsupported(what) => write!(f, "glTF uses unsupported {what}"),
            Self::InvalidGeometry(error) => write!(f, "glTF geometry invalid: {error}"),
        }
    }
}

impl std::error::Error for GltfError {}

impl GltfScene {
    /// Parses a `.glb` container or a `.gltf` JSON document whose buffer is
    /// embedded as a base64 data URI.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, GltfError> {
        if bytes.is_empty() {
            return Err(GltfError::EmptyInput);
        }
        let parsed =
            gltf::Gltf::from_slice(bytes).map_err(|error| GltfError::Parse(error.to_string()))?;
        let bin = parsed.blob;
        let document = parsed.document;

        let mut decoded: Vec<Option<Vec<u8>>> = Vec::new();
        for buffer in document.buffers() {
            while decoded.len() <= buffer.index() {
                decoded.push(None);
            }
            if let Source::Uri(uri) = buffer.source() {
                decoded[buffer.index()] = Some(decode_data_uri(uri)?);
            }
        }
        let resolve = |buffer: gltf::Buffer<'_>| match buffer.source() {
            Source::Bin => bin.as_deref(),
            Source::Uri(_) => decoded.get(buffer.index()).and_then(|slot| slot.as_deref()),
        };

        let scene = document
            .default_scene()
            .or_else(|| document.scenes().next())
            .ok_or(GltfError::MissingData("scene"))?;

        let mut parts = Vec::new();
        let mut stack: Vec<(gltf::Node<'_>, Mat4)> =
            scene.nodes().map(|node| (node, Mat4::IDENTITY)).collect();
        let mut visited = HashSet::new();
        while let Some((node, parent)) = stack.pop() {
            if !visited.insert(node.index()) {
                continue;
            }
            let transform = parent * node_transform(&node);
            if let Some(mesh) = node.mesh() {
                for primitive in mesh.primitives() {
                    parts.push(bake_primitive(&primitive, transform, resolve)?);
                }
            }
            for child in node.children() {
                stack.push((child, transform));
            }
        }

        Ok(Self { meshes: parts })
    }

    /// Loads a glTF file from disk. Native only; browser builds should fetch
    /// the bytes themselves and call [`GltfScene::from_bytes`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, GltfError> {
        let bytes = std::fs::read(path).map_err(|_| GltfError::MissingData("file on disk"))?;
        Self::from_bytes(&bytes)
    }
}

fn node_transform(node: &gltf::Node<'_>) -> Mat4 {
    match node.transform() {
        gltf::scene::Transform::Matrix { matrix } => Mat4::from_cols_array_2d(&matrix),
        gltf::scene::Transform::Decomposed {
            translation,
            rotation,
            scale,
        } => {
            Mat4::from_rotation_translation(
                glam::Quat::from_array(rotation),
                Vec3::from_array(translation),
            ) * Mat4::from_scale(Vec3::from_array(scale))
        }
    }
}

fn bake_primitive<'a, 's, F>(
    primitive: &'a gltf::Primitive<'a>,
    transform: Mat4,
    get_buffer_data: F,
) -> Result<GltfMeshPart, GltfError>
where
    F: Clone + Fn(gltf::Buffer<'a>) -> Option<&'s [u8]>,
{
    let reader = primitive.reader(get_buffer_data);
    let mut positions: Vec<Vec3> = reader
        .read_positions()
        .ok_or(GltfError::MissingData("POSITION accessor"))?
        .map(Vec3::from)
        .collect();

    let indices: Vec<u32> = match reader.read_indices() {
        Some(iter) => iter.into_u32().collect(),
        None => (0..positions.len() as u32).collect(),
    };

    let normals: Vec<Vec3> = match reader.read_normals() {
        Some(iter) => iter.map(Vec3::from).collect(),
        None => flat_normals(&positions, &indices),
    };

    let uvs: Vec<Vec2> = match reader.read_tex_coords(0) {
        Some(iter) => iter.into_f32().map(Vec2::from).collect(),
        None => vec![Vec2::ZERO; positions.len()],
    };

    if normals.len() != positions.len() || uvs.len() != positions.len() {
        return Err(GltfError::MissingData("consistent vertex attributes"));
    }

    let normal_matrix = Mat3::from_mat4(transform).inverse().transpose();
    for position in &mut positions {
        *position = transform.transform_point3(*position);
    }
    let vertices: Vec<MeshVertex> = positions
        .iter()
        .zip(&normals)
        .zip(&uvs)
        .map(|((position, normal), uv)| MeshVertex {
            position: *position,
            normal: (normal_matrix * *normal).normalize_or_zero(),
            uv: *uv,
        })
        .collect();

    let mesh = Mesh3D::new(vertices, indices).map_err(GltfError::InvalidGeometry)?;

    let material = primitive.material();
    let pbr = material.pbr_metallic_roughness();
    let base = pbr.base_color_factor();
    let emissive = material.emissive_factor();
    let material = Material3D {
        base_color: Color::rgba(base[0], base[1], base[2], base[3]),
        metallic: pbr.metallic_factor(),
        roughness: pbr.roughness_factor(),
        emissive: Color::rgb(emissive[0], emissive[1], emissive[2]),
    };

    Ok(GltfMeshPart { mesh, material })
}

/// Area-weighted per-vertex normals for geometry shipped without a NORMAL
/// accessor.
fn flat_normals(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for triangle in indices.chunks_exact(3) {
        let a = positions[triangle[0] as usize];
        let b = positions[triangle[1] as usize];
        let c = positions[triangle[2] as usize];
        let face = (b - a).cross(c - a);
        normals[triangle[0] as usize] += face;
        normals[triangle[1] as usize] += face;
        normals[triangle[2] as usize] += face;
    }
    normals.iter().map(|n| n.normalize_or_zero()).collect()
}

fn decode_data_uri(uri: &str) -> Result<Vec<u8>, GltfError> {
    let Some(encoded) = uri
        .strip_prefix("data:")
        .and_then(|rest| rest.find(',').map(|comma| &rest[comma + 1..]))
    else {
        return Err(GltfError::Unsupported("external buffer uri"));
    };
    base64_decode(encoded).ok_or_else(|| GltfError::Parse("invalid base64 buffer".to_string()))
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for byte in input.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        if byte == b'=' {
            break;
        }
        acc = (acc << 6) | value(byte)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON_CHUNK: u32 = 0x4E4F_534A;
    const BIN_CHUNK: u32 = 0x004E_4942;

    fn write_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn glb_from_chunks(json: &str, bin: &[u8]) -> Vec<u8> {
        let mut json_bytes = json.as_bytes().to_vec();
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(b' ');
        }
        let mut bin_bytes = bin.to_vec();
        while !bin_bytes.len().is_multiple_of(4) {
            bin_bytes.push(0);
        }
        let total = 12 + 8 + json_bytes.len() + 8 + bin_bytes.len();
        let mut out = Vec::with_capacity(total);
        write_u32(&mut out, 0x4654_6C67);
        write_u32(&mut out, 2);
        write_u32(&mut out, total as u32);
        write_u32(&mut out, json_bytes.len() as u32);
        write_u32(&mut out, JSON_CHUNK);
        out.extend_from_slice(&json_bytes);
        write_u32(&mut out, bin_bytes.len() as u32);
        write_u32(&mut out, BIN_CHUNK);
        out.extend_from_slice(&bin_bytes);
        out
    }

    fn triangle_bin(with_normals: bool) -> Vec<u8> {
        let positions: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let indices: [u32; 3] = [0, 1, 2];
        let mut bin = Vec::new();
        bin.extend_from_slice(bytemuck::cast_slice(&positions));
        if with_normals {
            let normals: [f32; 9] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
            bin.extend_from_slice(bytemuck::cast_slice(&normals));
        }
        bin.extend_from_slice(bytemuck::cast_slice(&indices));
        bin
    }

    fn triangle_json(node_extra: &str, with_normals: bool, buffer_uri: Option<&str>) -> String {
        let index_accessor = if with_normals { 2 } else { 1 };
        let index_view_offset = if with_normals { 72 } else { 36 };
        let normal_attribute = if with_normals { r#", "NORMAL": 1"# } else { "" };
        let normal_view = if with_normals {
            r#", {"buffer": 0, "byteOffset": 36, "byteLength": 36}"#
        } else {
            ""
        };
        let normal_accessor = if with_normals {
            r#", {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"}"#
        } else {
            ""
        };
        let buffer = match buffer_uri {
            Some(uri) => format!(r#"{{"byteLength": 84, "uri": "{uri}"}}"#),
            None => r#"{"byteLength": 84}"#.to_string(),
        };
        format!(
            r#"{{
                "asset": {{"version": "2.0"}},
                "scene": 0,
                "scenes": [{{"nodes": [0]}}],
                "nodes": [{{"mesh": 0{node_extra}}}],
                "buffers": [{buffer}],
                "bufferViews": [
                    {{"buffer": 0, "byteOffset": 0, "byteLength": 36}}{normal_view},
                    {{"buffer": 0, "byteOffset": {index_view_offset}, "byteLength": 12}}
                ],
                "accessors": [
                    {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                      "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]}}{normal_accessor},
                    {{"bufferView": {index_accessor}, "componentType": 5125, "count": 3,
                      "type": "SCALAR"}}
                ],
                "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0{normal_attribute}}},
                    "indices": {index_accessor}, "material": 0}}]}}],
                "materials": [{{"pbrMetallicRoughness": {{
                    "baseColorFactor": [0.5, 0.25, 0.125, 1.0],
                    "metallicFactor": 0.8, "roughnessFactor": 0.2}},
                    "emissiveFactor": [0.1, 0.2, 0.3]}}]
            }}"#
        )
    }

    fn base64_encode(data: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b0 = chunk[0];
            let b1 = chunk.get(1).copied().unwrap_or(0);
            let b2 = chunk.get(2).copied().unwrap_or(0);
            let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
            out.push(TABLE[(n >> 18) as usize & 63] as char);
            out.push(TABLE[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                TABLE[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                TABLE[n as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    #[test]
    fn glb_triangle_parses_with_baked_node_transform() {
        let json = triangle_json(r#", "translation": [1.0, 2.0, 3.0]"#, true, None);
        let bytes = glb_from_chunks(&json, &triangle_bin(true));

        let scene = GltfScene::from_bytes(&bytes).expect("valid GLB parses");
        assert_eq!(scene.meshes.len(), 1);
        let part = &scene.meshes[0];
        assert_eq!(part.mesh.vertices.len(), 3);
        assert_eq!(part.mesh.indices, vec![0, 1, 2]);
        assert_eq!(part.mesh.vertices[0].position, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(part.mesh.vertices[1].position, Vec3::new(2.0, 2.0, 3.0));
        assert_eq!(part.mesh.vertices[0].normal, Vec3::Z);
        assert_eq!(part.material.base_color, Color::rgba(0.5, 0.25, 0.125, 1.0));
        assert_eq!(part.material.metallic, 0.8);
        assert_eq!(part.material.roughness, 0.2);
        assert_eq!(part.material.emissive, Color::rgb(0.1, 0.2, 0.3));
    }

    #[test]
    fn gltf_with_embedded_base64_buffer_parses() {
        let json = triangle_json(
            "",
            true,
            Some("data:application/octet-stream;base64,PLACEHOLDER"),
        );
        let json = json.replace(
            "data:application/octet-stream;base64,PLACEHOLDER",
            &format!(
                "data:application/octet-stream;base64,{}",
                base64_encode(&triangle_bin(true))
            ),
        );

        let scene = GltfScene::from_bytes(json.as_bytes()).expect("embedded base64 buffer parses");
        assert_eq!(scene.meshes.len(), 1);
        assert_eq!(scene.meshes[0].mesh.vertices.len(), 3);
    }

    #[test]
    fn missing_normal_accessor_gets_flat_normals() {
        let json = triangle_json("", false, None);
        let bytes = glb_from_chunks(&json, &triangle_bin(false));

        let scene = GltfScene::from_bytes(&bytes).expect("GLB without normals parses");
        for vertex in &scene.meshes[0].mesh.vertices {
            assert_eq!(vertex.normal, Vec3::Z);
        }
    }

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(GltfScene::from_bytes(&[]), Err(GltfError::EmptyInput));
    }

    #[test]
    fn bad_magic_falls_back_to_json_and_errors() {
        let bytes = b"not a glTF file at all".to_vec();
        assert!(matches!(
            GltfScene::from_bytes(&bytes),
            Err(GltfError::Parse(_))
        ));
    }

    #[test]
    fn external_buffer_uris_are_unsupported() {
        let json = triangle_json("", true, Some("scene.bin"));
        assert!(matches!(
            GltfScene::from_bytes(json.as_bytes()),
            Err(GltfError::Unsupported(_))
        ));
    }
}
