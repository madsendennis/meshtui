//! mmap-based mesh loaders: STL (binary + ASCII), OBJ, PLY (ASCII + binary),
//! Draco (.drc) and GLB (binary glTF, incl. Draco-compressed primitives).

mod drc;
mod glb;
mod obj;
mod ply;
mod stl;

use std::path::{Path, PathBuf};

use memmap2::Mmap;
use thiserror::Error;

use crate::mesh::Mesh;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("failed to read {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("unsupported mesh format: {0}")]
    UnsupportedFormat(String),
    #[error("malformed {format} file {path}: {reason}")]
    Malformed {
        path: PathBuf,
        format: &'static str,
        reason: String,
    },
    #[error("empty mesh file {0}")]
    Empty(PathBuf),
}

impl LoadError {
    pub(crate) fn malformed(path: &Path, format: &'static str, reason: impl Into<String>) -> Self {
        Self::Malformed {
            path: path.to_path_buf(),
            format,
            reason: reason.into(),
        }
    }
}

/// Load one or more meshes from a file. Multi-geometry files (e.g. OBJ with
/// multiple objects) return all geometries — unlike the Python version, which
/// silently dropped everything after the first.
pub fn load_meshes(path: &Path) -> Result<Vec<Mesh>, LoadError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let file = std::fs::File::open(path).map_err(|e| LoadError::Io(path.to_path_buf(), e))?;
    // SAFETY: we never write through the mapping; file is only read.
    let map = unsafe { Mmap::map(&file) }.map_err(|e| LoadError::Io(path.to_path_buf(), e))?;
    if map.is_empty() {
        return Err(LoadError::Empty(path.to_path_buf()));
    }
    let bytes: &[u8] = &map;
    let mut meshes = match ext.as_str() {
        "stl" => vec![stl::load(path, bytes)?],
        "obj" => obj::load(path, bytes)?,
        "ply" => ply::load(path, bytes)?,
        "drc" => drc::load(path, bytes)?,
        "glb" => glb::load(path, bytes)?,
        other => return Err(LoadError::UnsupportedFormat(other.to_string())),
    };
    for m in &mut meshes {
        validate_mesh(path, m)?;
        if m.normals.is_empty()
            || m.normals
                .iter()
                .any(|normal| normal.length_squared() <= f32::EPSILON)
        {
            m.compute_normals();
        } else {
            for normal in &mut m.normals {
                *normal = normal.normalize();
            }
        }
        validate_mesh(path, m)?;
    }
    Ok(meshes)
}

fn validate_mesh(path: &Path, mesh: &Mesh) -> Result<(), LoadError> {
    if mesh.positions.iter().any(|p| !p.is_finite()) {
        return Err(LoadError::malformed(
            path,
            "mesh",
            "vertex positions must be finite",
        ));
    }
    if !mesh.indices.len().is_multiple_of(3) {
        return Err(LoadError::malformed(
            path,
            "mesh",
            "index count is not a multiple of 3",
        ));
    }
    if let Some(index) = mesh
        .indices
        .iter()
        .copied()
        .find(|&index| index as usize >= mesh.positions.len())
    {
        return Err(LoadError::malformed(
            path,
            "mesh",
            format!(
                "vertex index {index} is out of range for {} vertices",
                mesh.positions.len()
            ),
        ));
    }
    if !mesh.normals.is_empty() {
        if mesh.normals.len() != mesh.positions.len() {
            return Err(LoadError::malformed(
                path,
                "mesh",
                "normal count does not match vertex count",
            ));
        }
        if mesh.normals.iter().any(|n| !n.is_finite()) {
            return Err(LoadError::malformed(
                path,
                "mesh",
                "vertex normals must be finite",
            ));
        }
    }
    Ok(())
}

pub(crate) fn mesh_name(path: &Path, suffix: Option<&str>) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("mesh");
    match suffix {
        Some(s) => format!("{stem}:{s}"),
        None => stem.to_string(),
    }
}
