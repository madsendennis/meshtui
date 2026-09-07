use std::path::Path;

use glam::Vec3;

use super::{mesh_name, LoadError};
use crate::mesh::Mesh;

/// Wavefront OBJ: v/vn/f with o/g grouping. Each object/group becomes a
/// separate Mesh. Supports polygon faces via fan triangulation and
/// negative (relative) indices.
pub fn load(path: &Path, bytes: &[u8]) -> Result<Vec<Mesh>, LoadError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| LoadError::malformed(path, "OBJ", "not valid UTF-8"))?;

    let mut positions: Vec<Vec3> = Vec::new();
    let mut normals: Vec<Vec3> = Vec::new();
    let mut meshes: Vec<Mesh> = Vec::new();
    let mut current: Option<Mesh> = None;

    let finish = |m: Option<Mesh>, meshes: &mut Vec<Mesh>| {
        if let Some(m) = m {
            if !m.is_empty() {
                meshes.push(m);
            }
        }
    };

    for (lineno, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("");
        let mut tok = line.split_whitespace();
        let Some(kw) = tok.next() else { continue };
        match kw {
            "v" => {
                let v = parse_f32s(&mut tok, 3).ok_or_else(|| err(path, lineno, "bad vertex"))?;
                positions.push(Vec3::new(v[0], v[1], v[2]));
            }
            "vn" => {
                let v = parse_f32s(&mut tok, 3).ok_or_else(|| err(path, lineno, "bad normal"))?;
                normals.push(Vec3::new(v[0], v[1], v[2]));
            }
            "o" | "g" => {
                finish(current.take(), &mut meshes);
                let name = tok.next().unwrap_or("unnamed");
                current = Some(Mesh::new(mesh_name(path, Some(name))));
            }
            "f" => {
                let m = current.get_or_insert_with(|| Mesh::new(mesh_name(path, None)));
                let mut face: Vec<(u32, Option<u32>)> = Vec::with_capacity(4);
                for part in tok {
                    let mut it = part.split('/');
                    let vi = resolve_index(it.next(), positions.len())
                        .ok_or_else(|| err(path, lineno, "bad face index"))?;
                    let ni = match it.nth(1) {
                        Some("") | None => None,
                        Some(s) => Some(
                            resolve_index(Some(s), normals.len())
                                .ok_or_else(|| err(path, lineno, "bad normal index"))?,
                        ),
                    };
                    face.push((vi, ni));
                }
                if face.len() < 3 {
                    return Err(err(path, lineno, "face with fewer than 3 vertices"));
                }
                // Remap global indices to per-mesh local indices.
                let base = m.positions.len() as u32;
                for &(vi, ni) in &face {
                    m.positions.push(positions[vi as usize]);
                    match ni {
                        Some(ni) => m.normals.push(normals[ni as usize]),
                        None => m.normals.push(Vec3::ZERO), // fixed by compute_normals
                    }
                }
                for i in 1..face.len() as u32 - 1 {
                    m.indices.extend_from_slice(&[base, base + i, base + i + 1]);
                }
            }
            _ => {} // mtllib, usemtl, vt, s: ignored
        }
    }
    finish(current.take(), &mut meshes);

    if meshes.is_empty() {
        return Err(LoadError::malformed(path, "OBJ", "no geometry found"));
    }
    // Recompute the whole mesh if any face vertex omitted a normal. Mixing
    // authored normals with zero placeholders corrupts interpolated lighting.
    for m in &mut meshes {
        if m.normals.contains(&Vec3::ZERO) {
            m.normals.clear();
        }
    }
    Ok(meshes)
}

fn resolve_index(s: Option<&str>, len: usize) -> Option<u32> {
    let raw: i64 = s?.parse().ok()?;
    let idx = if raw < 0 { len as i64 + raw } else { raw - 1 };
    (0..len as i64).contains(&idx).then_some(idx as u32)
}

fn parse_f32s<'a>(tok: &mut impl Iterator<Item = &'a str>, n: usize) -> Option<Vec<f32>> {
    let v: Option<Vec<f32>> = (0..n).map(|_| tok.next()?.parse().ok()).collect();
    v
}

fn err(path: &Path, lineno: usize, reason: &str) -> LoadError {
    LoadError::malformed(path, "OBJ", format!("line {}: {reason}", lineno + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_triangle() {
        let obj = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        let meshes = load(Path::new("t.obj"), obj).unwrap();
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].triangle_count(), 1);
    }

    #[test]
    fn quad_fan_and_groups() {
        let obj = b"v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\ng a\nf 1 2 3 4\ng b\nf -4 -3 -2\n";
        let meshes = load(Path::new("t.obj"), obj).unwrap();
        assert_eq!(meshes.len(), 2);
        assert_eq!(meshes[0].triangle_count(), 2);
        assert_eq!(meshes[1].triangle_count(), 1);
        assert!(meshes[0].name.ends_with(":a"));
    }

    #[test]
    fn normals_from_file() {
        let obj = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nvn 0 0 1\nf 1//1 2//1 3//1\n";
        let meshes = load(Path::new("t.obj"), obj).unwrap();
        assert_eq!(meshes[0].normals[0], Vec3::Z);
    }

    #[test]
    fn mixed_missing_normals_trigger_recompute() {
        let obj = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nvn 0 0 1\nf 1//1 2//1 3\n";
        let mut meshes = load(Path::new("t.obj"), obj).unwrap();
        assert!(meshes[0].normals.is_empty());
        meshes[0].compute_normals();
        assert!(meshes[0].normals.iter().all(|n| n.dot(Vec3::Z) > 0.999));
    }

    #[test]
    fn out_of_range_index_is_error() {
        let obj = b"v 0 0 0\nf 1 2 9\n";
        assert!(load(Path::new("t.obj"), obj).is_err());
    }
}
