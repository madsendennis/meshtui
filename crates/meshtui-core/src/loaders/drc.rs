use std::path::Path;

use draco_oxide::core::types::{PointIdx, Vector};
use draco_oxide::{AttributeDomain, AttributeType, ComponentDataType, NdVector};

use super::{mesh_name, LoadError};
use crate::mesh::Mesh;

/// Draco (.drc) loader, via the pure-Rust draco-oxide decoder.
pub fn load(path: &Path, bytes: &[u8]) -> Result<Vec<Mesh>, LoadError> {
    let mesh = draco_oxide::decode::decode_mesh(bytes)
        .map_err(|e| LoadError::malformed(path, "DRC", format!("decode failed: {e}")))?;
    Ok(vec![from_draco_mesh(path, &mesh, mesh_name(path, None))?])
}

/// Convert a decoded draco-oxide mesh into our flat triangle mesh.
pub(crate) fn from_draco_mesh(
    path: &Path,
    src: &draco_oxide::Mesh,
    name: String,
) -> Result<Mesh, LoadError> {
    let pos = src
        .attributes
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Position)
        .ok_or_else(|| LoadError::malformed(path, "DRC", "no position attribute"))?;
    if pos.get_component_type() != ComponentDataType::F32 || pos.get_num_components() < 3 {
        return Err(LoadError::malformed(
            path,
            "DRC",
            "position attribute is not float32 vec3",
        ));
    }

    let mut mesh = Mesh::new(name);
    let num_points = pos.len();
    mesh.positions.reserve(num_points);
    for p in 0..num_points {
        let v = pos.get::<NdVector<3, f32>, 3>(PointIdx::from(p));
        mesh.positions
            .push(glam::Vec3::new(*v.get(0), *v.get(1), *v.get(2)));
    }

    if let Some(normals) = src
        .attributes
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Normal)
    {
        if normals.get_domain() == AttributeDomain::Position
            && normals.get_component_type() == ComponentDataType::F32
            && normals.get_num_components() >= 3
            && normals.len() == num_points
        {
            mesh.normals.reserve(num_points);
            for p in 0..num_points {
                let v = normals.get::<NdVector<3, f32>, 3>(PointIdx::from(p));
                mesh.normals
                    .push(glam::Vec3::new(*v.get(0), *v.get(1), *v.get(2)));
            }
        }
    }

    if let Some(color) = src
        .attributes
        .iter()
        .find(|a| a.get_attribute_type() == AttributeType::Color)
    {
        if color.get_domain() == AttributeDomain::Position
            && color.get_component_type() == ComponentDataType::F32
            && color.get_num_components() >= 3
            && color.len() == num_points
            && num_points > 0
        {
            let components = color.get_num_components();
            let mut acc = [0f64; 4];
            for p in 0..num_points {
                if components >= 4 {
                    let v = color.get::<NdVector<4, f32>, 4>(PointIdx::from(p));
                    acc[0] += *v.get(0) as f64;
                    acc[1] += *v.get(1) as f64;
                    acc[2] += *v.get(2) as f64;
                    acc[3] += *v.get(3) as f64;
                } else {
                    let v = color.get::<NdVector<3, f32>, 3>(PointIdx::from(p));
                    acc[0] += *v.get(0) as f64;
                    acc[1] += *v.get(1) as f64;
                    acc[2] += *v.get(2) as f64;
                    acc[3] += 1.0;
                }
            }
            let n = num_points as f64;
            let c = [
                (acc[0] / n) as f32,
                (acc[1] / n) as f32,
                (acc[2] / n) as f32,
                (acc[3] / n) as f32,
            ];
            mesh.original_color = Some(c);
            mesh.color = c;
        }
    }

    mesh.indices.reserve(src.get_faces().len() * 3);
    for face in src.get_faces() {
        mesh.indices.push(usize::from(face[0]) as u32);
        mesh.indices.push(usize::from(face[1]) as u32);
        mesh.indices.push(usize::from(face[2]) as u32);
    }

    if mesh.is_empty() {
        return Err(LoadError::malformed(path, "DRC", "no triangles decoded"));
    }
    Ok(mesh)
}
