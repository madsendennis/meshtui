use std::path::Path;

use draco_oxide::io::gltf::draco_extension::{is_draco_compressed, is_triangle_primitive};
use draco_oxide::io::gltf::geometry_extractor::{
    get_accessor_info, read_accessor_as_u32, read_accessor_as_vec3, read_accessor_as_vec4,
    ComponentType,
};
use draco_oxide::io::gltf::glb::parse_glb;
use glam::Vec3;
use serde_json::Value;

use super::{drc, mesh_name, LoadError};
use crate::mesh::Mesh;

/// GLB (binary glTF) loader. Extracts every mesh's triangle primitives with
/// POSITION/NORMAL/COLOR_0 attributes; supports primitives compressed with
/// KHR_draco_mesh_compression. Node transforms are not applied (the Python
/// version only extracted the first geometry of a scene; we load all meshes
/// but keep their local coordinates).
pub fn load(path: &Path, bytes: &[u8]) -> Result<Vec<Mesh>, LoadError> {
    let err = |reason: String| LoadError::malformed(path, "GLB", reason);
    let glb = parse_glb(bytes).map_err(|e| err(format!("bad GLB container: {e}")))?;
    let json: Value =
        serde_json::from_slice(&glb.json).map_err(|e| err(format!("bad glTF JSON: {e}")))?;
    let buffer = glb.buffer.as_slice();

    let meshes_json = json["meshes"]
        .as_array()
        .ok_or_else(|| err("no meshes in glTF".to_string()))?;

    let mut meshes = Vec::new();
    for (mi, mesh_json) in meshes_json.iter().enumerate() {
        let name = mesh_json["name"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| mesh_name(path, (mi > 0).then(|| mi.to_string()).as_deref()));
        let mut mesh = Mesh::new(name);
        let primitives = mesh_json["primitives"]
            .as_array()
            .ok_or_else(|| err(format!("mesh {mi} has no primitives")))?;
        for primitive in primitives {
            if !is_triangle_primitive(primitive) {
                continue; // points/lines are not renderable meshes
            }
            load_primitive(path, &json, buffer, primitive, &mut mesh)?;
        }
        if !mesh.is_empty() {
            meshes.push(mesh);
        }
    }
    if meshes.is_empty() {
        return Err(LoadError::malformed(path, "GLB", "no triangles parsed"));
    }
    Ok(meshes)
}

fn load_primitive(
    path: &Path,
    json: &Value,
    buffer: &[u8],
    primitive: &Value,
    mesh: &mut Mesh,
) -> Result<(), LoadError> {
    let err = |reason: String| LoadError::malformed(path, "GLB", reason);

    if is_draco_compressed(primitive) {
        let ext = &primitive["extensions"]["KHR_draco_mesh_compression"];
        let view_idx = ext["bufferView"]
            .as_u64()
            .ok_or_else(|| err("draco primitive without bufferView".to_string()))?;
        let view = json["bufferViews"]
            .get(view_idx as usize)
            .ok_or_else(|| err(format!("bufferView {view_idx} not found")))?;
        let start = view["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = view["byteLength"]
            .as_u64()
            .ok_or_else(|| err("bufferView without byteLength".to_string()))?
            as usize;
        let data = buffer
            .get(start..start + len)
            .ok_or_else(|| err("draco bufferView out of bounds".to_string()))?;
        let decoded = draco_oxide::decode::decode_mesh(data)
            .map_err(|e| err(format!("draco decode failed: {e}")))?;
        let mut part = drc::from_draco_mesh(path, &decoded, mesh.name.clone())?;
        mesh.positions.append(&mut part.positions);
        // Normals/colors from separate primitives can't be merged index-wise;
        // keep them only when the target mesh is still empty of vertices.
        if mesh.indices.is_empty() {
            mesh.normals.append(&mut part.normals);
            if let Some(c) = part.original_color {
                mesh.original_color = Some(c);
                mesh.color = c;
            }
        }
        mesh.indices.append(&mut part.indices);
        return Ok(());
    }

    let attributes = &primitive["attributes"];
    let pos_idx = attributes["POSITION"]
        .as_u64()
        .ok_or_else(|| err("primitive without POSITION".to_string()))?;
    let base = mesh.positions.len() as u32;

    let positions =
        read_accessor_as_vec3(json, buffer, pos_idx).map_err(|e| err(format!("POSITION: {e}")))?;
    mesh.positions
        .extend(positions.iter().map(|p| Vec3::from(*p)));

    if let Some(n_idx) = attributes["NORMAL"].as_u64() {
        let normals =
            read_accessor_as_vec3(json, buffer, n_idx).map_err(|e| err(format!("NORMAL: {e}")))?;
        if normals.len() == positions.len() {
            mesh.normals.extend(normals.iter().map(|n| Vec3::from(*n)));
        }
    }

    if let Some(c_idx) = attributes["COLOR_0"].as_u64() {
        // Average vertex color, matching the PLY loader behavior.
        if let Some(c) =
            read_color(json, buffer, c_idx).map_err(|e| err(format!("COLOR_0: {e}")))?
        {
            mesh.original_color = Some(c);
            mesh.color = c;
        }
    } else if let Some(material_idx) = primitive["material"].as_u64() {
        if let Some(factor) = json["materials"]
            .get(material_idx as usize)
            .and_then(|m| m["pbrMetallicRoughness"]["baseColorFactor"].as_array())
        {
            let c: Vec<f32> = factor
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            if c.len() == 4 {
                mesh.original_color = Some([c[0], c[1], c[2], c[3]]);
                mesh.color = [c[0], c[1], c[2], c[3]];
            }
        }
    }

    match primitive["indices"].as_u64() {
        Some(i_idx) => {
            let indices = read_accessor_as_u32(json, buffer, i_idx)
                .map_err(|e| err(format!("indices: {e}")))?;
            mesh.indices.extend(indices.iter().map(|i| base + i));
        }
        None => mesh
            .indices
            .extend((0..positions.len() as u32).map(|i| base + i)),
    }
    Ok(())
}

/// Average of a COLOR_0 accessor as RGBA (integer components are normalized
/// per the glTF spec).
fn read_color(
    json: &Value,
    buffer: &[u8],
    accessor_idx: u64,
) -> Result<Option<[f32; 4]>, draco_oxide::io::gltf::geometry_extractor::Error> {
    let info = get_accessor_info(json, accessor_idx)?;
    let scale = match info.component_type {
        ComponentType::Float => 1.0,
        // glTF requires normalized integer colors.
        ComponentType::UnsignedByte => u8::MAX as f32,
        ComponentType::UnsignedShort => u16::MAX as f32,
        _ => return Ok(None),
    };
    let mut acc = [0f64; 4];
    let count;
    match info.accessor_type.as_str() {
        "VEC3" => {
            let vals = read_accessor_as_vec3(json, buffer, accessor_idx)?;
            count = vals.len();
            for v in vals {
                acc[0] += (v[0] / scale) as f64;
                acc[1] += (v[1] / scale) as f64;
                acc[2] += (v[2] / scale) as f64;
                acc[3] += 1.0;
            }
        }
        "VEC4" => {
            let vals = read_accessor_as_vec4(json, buffer, accessor_idx)?;
            count = vals.len();
            for v in vals {
                acc[0] += (v[0] / scale) as f64;
                acc[1] += (v[1] / scale) as f64;
                acc[2] += (v[2] / scale) as f64;
                acc[3] += (v[3] / scale) as f64;
            }
        }
        _ => return Ok(None),
    }
    if count == 0 {
        return Ok(None);
    }
    let n = count as f64;
    Ok(Some([
        (acc[0] / n) as f32,
        (acc[1] / n) as f32,
        (acc[2] / n) as f32,
        (acc[3] / n) as f32,
    ]))
}
