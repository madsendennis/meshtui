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
    #[error("no supported meshes (*.ply, *.stl, *.obj, *.drc, *.glb) in {0}")]
    NoMeshes(PathBuf),
}

/// Extensions recognized by the loaders, lower-case.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["stl", "obj", "ply", "drc", "glb"];

/// Like [`load_path`], but each mesh carries the file it was loaded from
/// (one input file can hold several meshes, e.g. multi-object OBJ).
pub fn load_path_detailed(path: &Path) -> Result<Vec<(PathBuf, Mesh)>, LoadError> {
    if !path.is_dir() {
        return Ok(load_meshes(path)?
            .into_iter()
            .map(|mesh| (path.to_path_buf(), mesh))
            .collect());
    }
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path).map_err(|e| LoadError::Io(path.to_path_buf(), e))? {
        let entry = entry.map_err(|e| LoadError::Io(path.to_path_buf(), e))?;
        let entry_path = entry.path();
        if entry_path.is_file()
            && entry_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
        {
            entries.push(entry_path);
        }
    }
    entries.sort();
    let mut meshes = Vec::new();
    for entry_path in &entries {
        meshes.extend(
            load_meshes(entry_path)?
                .into_iter()
                .map(|mesh| (entry_path.clone(), mesh)),
        );
    }
    if meshes.is_empty() {
        return Err(LoadError::NoMeshes(path.to_path_buf()));
    }
    Ok(meshes)
}

/// Load a single mesh file, or every supported mesh inside a directory
/// (sorted, non-recursive). Shared by the CLI and the interactive `open`
/// prompt so both behave identically.
pub fn load_path(path: &Path) -> Result<Vec<Mesh>, LoadError> {
    Ok(load_path_detailed(path)?
        .into_iter()
        .map(|(_, mesh)| mesh)
        .collect())
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
    for (source_index, m) in meshes.iter_mut().enumerate() {
        m.source = Some(path.to_path_buf());
        m.source_index = Some(source_index);
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
