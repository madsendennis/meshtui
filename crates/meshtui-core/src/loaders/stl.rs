use std::path::Path;

use glam::Vec3;

use super::{mesh_name, LoadError};
use crate::mesh::Mesh;

/// Binary STL header is 80 bytes + u32 triangle count; ASCII starts with
/// "solid" and contains "facet". Detect robustly: prefer binary when the
/// declared size matches the file size.
pub fn load(path: &Path, bytes: &[u8]) -> Result<Mesh, LoadError> {
    if let Some(mesh) = try_binary(path, bytes)? {
        return Ok(mesh);
    }
    ascii(path, bytes)
}

fn try_binary(path: &Path, bytes: &[u8]) -> Result<Option<Mesh>, LoadError> {
    if bytes.len() < 84 {
        return Ok(None);
    }
    let declared = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    // Some writers emit a wrong triangle count or trailing garbage; accept the
    // file as binary when the payload fits as many 50-byte records as it can,
    // as long as it's not plausibly ASCII (which starts with "solid").
    let fits = (bytes.len() - 84) / 50;
    if fits == 0 || declared == 0 {
        return Ok(None);
    }
    if bytes.starts_with(b"solid") && 84 + declared * 50 != bytes.len() {
        return Ok(None); // probably ASCII that happens to be long enough
    }
    let tri_count = declared.min(fits);
    let mut mesh = Mesh::new(mesh_name(path, None));
    mesh.positions = Vec::with_capacity(tri_count * 3);
    mesh.normals = Vec::with_capacity(tri_count * 3);
    mesh.indices = Vec::with_capacity(tri_count * 3);
    for i in 0..tri_count {
        let base = 84 + i * 50;
        let rec = &bytes[base..base + 50];
        let normal = read_vec3(&rec[0..12]);
        for v in 0..3 {
            let off = 12 + v * 12;
            mesh.positions.push(read_vec3(&rec[off..off + 12]));
            mesh.normals.push(normal);
        }
        let i = (i * 3) as u32;
        mesh.indices.extend_from_slice(&[i, i + 1, i + 2]);
    }
    Ok(Some(mesh))
}

fn read_vec3(b: &[u8]) -> Vec3 {
    Vec3::new(
        f32::from_le_bytes(b[0..4].try_into().unwrap()),
        f32::from_le_bytes(b[4..8].try_into().unwrap()),
        f32::from_le_bytes(b[8..12].try_into().unwrap()),
    )
}

fn ascii(path: &Path, bytes: &[u8]) -> Result<Mesh, LoadError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| LoadError::malformed(path, "STL", "not valid UTF-8 or binary STL"))?;
    let mut mesh = Mesh::new(mesh_name(path, None));
    let mut normal = Vec3::Z;
    for line in text.lines() {
        let mut tok = line.split_whitespace();
        match tok.next() {
            Some("facet") => {
                // facet normal nx ny nz
                if tok.next() == Some("normal") {
                    normal = parse_vec3(&mut tok)
                        .ok_or_else(|| LoadError::malformed(path, "STL", "bad facet normal"))?;
                }
            }
            Some("vertex") => {
                let v = parse_vec3(&mut tok)
                    .ok_or_else(|| LoadError::malformed(path, "STL", "bad vertex"))?;
                mesh.positions.push(v);
                mesh.normals.push(normal);
            }
            _ => {}
        }
    }
    let n = mesh.positions.len();
    if n == 0 || !n.is_multiple_of(3) {
        return Err(LoadError::malformed(
            path,
            "STL",
            format!("vertex count {n} is not a multiple of 3"),
        ));
    }
    mesh.indices = (0..n as u32).collect();
    Ok(mesh)
}

fn parse_vec3<'a>(tok: &mut impl Iterator<Item = &'a str>) -> Option<Vec3> {
    Some(Vec3::new(
        tok.next()?.parse().ok()?,
        tok.next()?.parse().ok()?,
        tok.next()?.parse().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_stl_roundtrip() {
        let mut bytes = vec![0u8; 80];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        // normal (0,0,1), verts (0,0,0) (1,0,0) (0,1,0), attr 0
        for f in [
            0.0f32, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ] {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        let m = load(Path::new("t.stl"), &bytes).unwrap();
        assert_eq!(m.triangle_count(), 1);
        assert_eq!(m.positions[2], Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn ascii_stl() {
        let text = b"solid t
facet normal 0 0 1
outer loop
vertex 0 0 0
vertex 1 0 0
vertex 0 1 0
endloop
endfacet
endsolid t";
        let m = load(Path::new("t.stl"), text).unwrap();
        assert_eq!(m.triangle_count(), 1);
    }

    #[test]
    fn wrong_triangle_count_is_tolerated() {
        // Real-world files (e.g. Meshmixer exports) may declare a count that
        // doesn't match the payload; load what actually fits.
        let mut bytes = b"MESHMIXER-STL-BINARY-FORMAT".to_vec();
        bytes.resize(80, b'-');
        bytes.extend_from_slice(&5u32.to_le_bytes()); // declares 5, contains 1
        for f in [
            0.0f32, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
        ] {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        let m = load(Path::new("t.stl"), &bytes).unwrap();
        assert_eq!(m.triangle_count(), 1);
    }

    #[test]
    fn garbage_is_error_not_panic() {
        assert!(load(Path::new("t.stl"), b"not an stl").is_err());
    }
}
