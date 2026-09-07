use std::path::Path;

use glam::Vec3;

use super::{mesh_name, LoadError};
use crate::mesh::Mesh;

/// PLY loader: ASCII, binary_little_endian and binary_big_endian formats.
///
/// The header is parsed into a generic element/property model, so files with
/// extra properties (e.g. trimesh writes a scalar `stl` property on faces) or
/// extra elements are handled: the `vertex` element contributes x/y/z
/// (+ optional nx/ny/nz, red/green/blue/alpha), the `face` element contributes
/// its vertex_indices/vertex_index list, and everything else is skipped.
pub fn load(path: &Path, bytes: &[u8]) -> Result<Vec<Mesh>, LoadError> {
    let (fmt, elements, body) = parse_header(path, bytes)?;

    let mut mesh = Mesh::new(mesh_name(path, None));

    // Per-element extraction plans.
    enum Plan {
        Vertex(VertexPlan),
        Face { index_list: Option<usize> },
        Skip,
    }
    struct VertexPlan {
        ix: usize,
        iy: usize,
        iz: usize,
        normals: Option<(usize, usize, usize)>,
        color: Option<(usize, usize, usize, Option<usize>)>,
    }

    let mut plans = Vec::with_capacity(elements.len());
    for elem in &elements {
        let plan = match elem.name.as_str() {
            "vertex" => {
                let idx = |name: &str| {
                    elem.props
                        .iter()
                        .position(|p| matches!(p, Prop::Scalar { name: n, .. } if n == name))
                };
                let (Some(ix), Some(iy), Some(iz)) = (idx("x"), idx("y"), idx("z")) else {
                    return Err(LoadError::malformed(
                        path,
                        "PLY",
                        "vertex element missing x/y/z",
                    ));
                };
                let normals = match (idx("nx"), idx("ny"), idx("nz")) {
                    (Some(a), Some(b), Some(c)) => Some((a, b, c)),
                    _ => None,
                };
                let color = match (idx("red"), idx("green"), idx("blue")) {
                    (Some(r), Some(g), Some(b)) => Some((r, g, b, idx("alpha"))),
                    _ => None,
                };
                Plan::Vertex(VertexPlan {
                    ix,
                    iy,
                    iz,
                    normals,
                    color,
                })
            }
            "face" => {
                let index_list = elem.props.iter().position(|p| {
                    matches!(
                        p,
                        Prop::List { name, .. }
                            if name == "vertex_indices" || name == "vertex_index"
                    )
                });
                if elem.count > 0 && index_list.is_none() {
                    return Err(LoadError::malformed(
                        path,
                        "PLY",
                        "face element without a vertex index list",
                    ));
                }
                Plan::Face { index_list }
            }
            _ => Plan::Skip,
        };
        plans.push(plan);
    }

    let mut color_acc = [0f64; 4];
    let mut color_n = 0u64;
    let mut has_normals = false;

    let mut visit = |ei: usize, vals: &[Val]| -> Result<(), LoadError> {
        let elem = &elements[ei];
        match &plans[ei] {
            Plan::Skip => Ok(()),
            Plan::Vertex(plan) => {
                let scalar = |i: usize| -> Result<f64, LoadError> {
                    match vals.get(i) {
                        Some(Val::Scalar(v)) => Ok(*v),
                        _ => Err(LoadError::malformed(path, "PLY", "bad vertex record")),
                    }
                };
                mesh.positions.push(Vec3::new(
                    scalar(plan.ix)? as f32,
                    scalar(plan.iy)? as f32,
                    scalar(plan.iz)? as f32,
                ));
                if let Some((a, b, c)) = plan.normals {
                    mesh.normals.push(Vec3::new(
                        scalar(a)? as f32,
                        scalar(b)? as f32,
                        scalar(c)? as f32,
                    ));
                    has_normals = true;
                }
                if let Some((r, g, b, a)) = plan.color {
                    let channel = |i: usize| -> Option<f64> {
                        match &elem.props[i] {
                            Prop::Scalar { ty, .. } => color_channel(scalar(i).ok()?, ty),
                            _ => None,
                        }
                    };
                    let (Some(red), Some(green), Some(blue)) = (channel(r), channel(g), channel(b))
                    else {
                        return Err(LoadError::malformed(path, "PLY", "invalid vertex color"));
                    };
                    let alpha = a.and_then(channel).unwrap_or(1.0);
                    color_acc[0] += red;
                    color_acc[1] += green;
                    color_acc[2] += blue;
                    color_acc[3] += alpha;
                    color_n += 1;
                }
                Ok(())
            }
            Plan::Face { index_list } => {
                let Some(li) = index_list else { return Ok(()) };
                let Some(Val::List(items)) = vals.get(*li) else {
                    return Err(LoadError::malformed(path, "PLY", "bad face record"));
                };
                let n = items.len();
                if !(3..=1_000_000).contains(&n) {
                    return Err(LoadError::malformed(
                        path,
                        "PLY",
                        "implausible face vertex count",
                    ));
                }
                let mut idx = Vec::with_capacity(n);
                for &v in items {
                    idx.push(
                        scalar_to_u32(v).ok_or_else(|| {
                            LoadError::malformed(path, "PLY", "invalid face index")
                        })?,
                    );
                }
                for i in 1..n - 1 {
                    mesh.indices
                        .extend_from_slice(&[idx[0], idx[i], idx[i + 1]]);
                }
                Ok(())
            }
        }
    };

    match fmt {
        Fmt::Ascii => walk_ascii(path, body, &elements, &mut visit)?,
        Fmt::Binary(big_endian) => walk_binary(path, body, &elements, big_endian, &mut visit)?,
    }

    if !has_normals {
        mesh.normals.clear();
    }
    if color_n > 0 {
        let n = color_n as f64;
        let c = [
            (color_acc[0] / n) as f32,
            (color_acc[1] / n) as f32,
            (color_acc[2] / n) as f32,
            (color_acc[3] / n) as f32,
        ];
        mesh.original_color = Some(c);
        mesh.color = c;
    }

    if mesh.is_empty() {
        return Err(LoadError::malformed(path, "PLY", "no triangles parsed"));
    }
    Ok(vec![mesh])
}

#[derive(Clone, Copy, PartialEq)]
enum Fmt {
    Ascii,
    /// `true` for big-endian.
    Binary(bool),
}

#[derive(Debug)]
enum Prop {
    Scalar {
        ty: String,
        name: String,
    },
    List {
        count_ty: String,
        item_ty: String,
        name: String,
    },
}

#[derive(Debug)]
struct Element {
    name: String,
    count: usize,
    props: Vec<Prop>,
}

/// A parsed property value: scalar, or a list of numbers (vertex indices).
enum Val {
    Scalar(f64),
    List(Vec<f64>),
}

fn parse_header<'a>(
    path: &Path,
    bytes: &'a [u8],
) -> Result<(Fmt, Vec<Element>, &'a [u8]), LoadError> {
    let header_end = find_header_end(path, bytes)?;
    let header = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| LoadError::malformed(path, "PLY", "header not UTF-8"))?;
    let mut lines = header.lines();
    if lines.next().map(str::trim) != Some("ply") {
        return Err(LoadError::malformed(path, "PLY", "missing magic"));
    }

    let mut fmt = None;
    let mut elements: Vec<Element> = Vec::new();

    for line in lines {
        let mut tok = line.split_whitespace();
        match tok.next() {
            Some("format") => {
                fmt = Some(match tok.next() {
                    Some("ascii") => Fmt::Ascii,
                    Some("binary_little_endian") => Fmt::Binary(false),
                    Some("binary_big_endian") => Fmt::Binary(true),
                    Some(other) => {
                        return Err(LoadError::malformed(
                            path,
                            "PLY",
                            format!("unsupported format {other}"),
                        ))
                    }
                    None => return Err(LoadError::malformed(path, "PLY", "bad format line")),
                });
            }
            Some("element") => {
                let name = tok.next().unwrap_or("").to_string();
                let count: usize = tok
                    .next()
                    .and_then(|c| c.parse().ok())
                    .ok_or_else(|| LoadError::malformed(path, "PLY", "bad element count"))?;
                elements.push(Element {
                    name,
                    count,
                    props: Vec::new(),
                });
            }
            Some("property") => {
                let Some(elem) = elements.last_mut() else {
                    return Err(LoadError::malformed(
                        path,
                        "PLY",
                        "property before any element",
                    ));
                };
                let first = tok.next().unwrap_or("");
                if first == "list" {
                    let count_ty = tok.next().unwrap_or("").to_string();
                    let item_ty = tok.next().unwrap_or("").to_string();
                    let name = tok.next().unwrap_or("").to_string();
                    for ty in [&count_ty, &item_ty] {
                        if type_size(ty).is_none() {
                            return Err(LoadError::malformed(
                                path,
                                "PLY",
                                format!("unknown list property type {ty}"),
                            ));
                        }
                    }
                    elem.props.push(Prop::List {
                        count_ty,
                        item_ty,
                        name,
                    });
                } else {
                    if type_size(first).is_none() {
                        return Err(LoadError::malformed(
                            path,
                            "PLY",
                            format!("unknown property type {first}"),
                        ));
                    }
                    elem.props.push(Prop::Scalar {
                        ty: first.to_string(),
                        name: tok.next().unwrap_or("").to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    let fmt = fmt.ok_or_else(|| LoadError::malformed(path, "PLY", "no format line"))?;
    Ok((fmt, elements, &bytes[header_end..]))
}

fn walk_ascii(
    path: &Path,
    body: &[u8],
    elements: &[Element],
    visit: &mut impl FnMut(usize, &[Val]) -> Result<(), LoadError>,
) -> Result<(), LoadError> {
    let text = std::str::from_utf8(body)
        .map_err(|_| LoadError::malformed(path, "PLY", "body not UTF-8"))?;
    let mut lines = text.lines();
    for (ei, elem) in elements.iter().enumerate() {
        for r in 0..elem.count {
            let line = lines.next().ok_or_else(|| {
                LoadError::malformed(
                    path,
                    "PLY",
                    format!("unexpected EOF in element {:?}", elem.name),
                )
            })?;
            let mut tok = line.split_whitespace();
            let mut vals = Vec::with_capacity(elem.props.len());
            for prop in &elem.props {
                let mut next = || -> Result<f64, LoadError> {
                    tok.next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .ok_or_else(|| {
                            LoadError::malformed(
                                path,
                                "PLY",
                                format!("bad record {r} in element {:?}", elem.name),
                            )
                        })
                };
                match prop {
                    Prop::Scalar { .. } => vals.push(Val::Scalar(next()?)),
                    Prop::List { .. } => {
                        let n = next()?;
                        let n = scalar_to_u32(n).ok_or_else(|| {
                            LoadError::malformed(path, "PLY", "invalid list count")
                        })? as usize;
                        let mut items = Vec::with_capacity(n.min(1_000_000));
                        for _ in 0..n {
                            items.push(next()?);
                        }
                        vals.push(Val::List(items));
                    }
                }
            }
            visit(ei, &vals)?;
        }
    }
    Ok(())
}

fn walk_binary(
    path: &Path,
    body: &[u8],
    elements: &[Element],
    big_endian: bool,
    visit: &mut impl FnMut(usize, &[Val]) -> Result<(), LoadError>,
) -> Result<(), LoadError> {
    let mut off = 0usize;
    for (ei, elem) in elements.iter().enumerate() {
        for _ in 0..elem.count {
            let mut vals = Vec::with_capacity(elem.props.len());
            for prop in &elem.props {
                match prop {
                    Prop::Scalar { ty, .. } => {
                        let v = read_scalar(body, ty, &mut off, big_endian).ok_or_else(|| {
                            LoadError::malformed(path, "PLY", "truncated binary data")
                        })?;
                        vals.push(Val::Scalar(v));
                    }
                    Prop::List {
                        count_ty, item_ty, ..
                    } => {
                        let n = read_scalar(body, count_ty, &mut off, big_endian)
                            .and_then(scalar_to_u32)
                            .ok_or_else(|| {
                                LoadError::malformed(path, "PLY", "invalid list count")
                            })? as usize;
                        if n > 1_000_000 {
                            return Err(LoadError::malformed(
                                path,
                                "PLY",
                                "implausible list count",
                            ));
                        }
                        let mut items = Vec::with_capacity(n);
                        for _ in 0..n {
                            items.push(
                                read_scalar(body, item_ty, &mut off, big_endian).ok_or_else(
                                    || LoadError::malformed(path, "PLY", "truncated binary data"),
                                )?,
                            );
                        }
                        vals.push(Val::List(items));
                    }
                }
            }
            visit(ei, &vals)?;
        }
    }
    Ok(())
}

fn find_header_end(path: &Path, bytes: &[u8]) -> Result<usize, LoadError> {
    let mut start = 0;
    while start < bytes.len() {
        let line_end = bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|offset| start + offset + 1)
            .unwrap_or(bytes.len());
        let line = &bytes[start..line_end];
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line == b"end_header" {
            return Ok(line_end);
        }
        start = line_end;
    }
    Err(LoadError::malformed(path, "PLY", "no end_header"))
}

fn type_size(ty: &str) -> Option<usize> {
    Some(match ty {
        "char" | "int8" | "uchar" | "uint8" => 1,
        "short" | "int16" | "ushort" | "uint16" => 2,
        "int" | "int32" | "uint" | "uint32" | "float" | "float32" => 4,
        "double" | "float64" => 8,
        _ => return None,
    })
}

fn read_scalar(bytes: &[u8], ty: &str, off: &mut usize, big_endian: bool) -> Option<f64> {
    let size = type_size(ty)?;
    let b = bytes.get(*off..*off + size)?;
    *off += size;
    macro_rules! num {
        ($t:ty) => {
            if big_endian {
                <$t>::from_be_bytes(b.try_into().ok()?) as f64
            } else {
                <$t>::from_le_bytes(b.try_into().ok()?) as f64
            }
        };
    }
    Some(match ty {
        "char" | "int8" => b[0] as i8 as f64,
        "uchar" | "uint8" => b[0] as f64,
        "short" | "int16" => num!(i16),
        "ushort" | "uint16" => num!(u16),
        "int" | "int32" => num!(i32),
        "uint" | "uint32" => num!(u32),
        "float" | "float32" => num!(f32),
        "double" | "float64" => num!(f64),
        _ => return None,
    })
}

fn scalar_to_u32(value: f64) -> Option<u32> {
    (value.is_finite() && value.fract() == 0.0 && value >= 0.0 && value <= u32::MAX as f64)
        .then_some(value as u32)
}

fn color_channel(value: f64, ty: &str) -> Option<f64> {
    let scale = match ty {
        "char" | "int8" => i8::MAX as f64,
        "uchar" | "uint8" => u8::MAX as f64,
        "short" | "int16" => i16::MAX as f64,
        "ushort" | "uint16" => u16::MAX as f64,
        "int" | "int32" => i32::MAX as f64,
        "uint" | "uint32" => u32::MAX as f64,
        "float" | "float32" | "double" | "float64" => 1.0,
        _ => return None,
    };
    let normalized = value / scale;
    (normalized.is_finite() && (0.0..=1.0).contains(&normalized)).then_some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_ply() {
        let ply = b"ply\nformat ascii 1.0\nelement vertex 3\n\
property float x\nproperty float y\nproperty float z\n\
element face 1\nproperty list uchar int vertex_indices\nend_header\n\
0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n";
        let meshes = load(Path::new("t.ply"), ply).unwrap();
        assert_eq!(meshes[0].triangle_count(), 1);
        assert_eq!(meshes[0].positions[1], Vec3::X);
    }

    #[test]
    fn trimesh_style_face_with_extra_scalar_property() {
        // trimesh exports faces with an extra scalar property after the list.
        let mut bytes = b"ply\nformat binary_little_endian 1.0\ncomment trimesh\n\
element vertex 3\nproperty float x\nproperty float y\nproperty float z\n\
element face 1\nproperty list uchar int vertex_indices\nproperty ushort stl\nend_header\n"
            .to_vec();
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for f in p {
                bytes.extend_from_slice(&f.to_le_bytes());
            }
        }
        bytes.push(3u8);
        for i in [0i32, 1, 2] {
            bytes.extend_from_slice(&i.to_le_bytes());
        }
        bytes.extend_from_slice(&7u16.to_le_bytes()); // the `stl` scalar
        let meshes = load(Path::new("t.ply"), &bytes).unwrap();
        assert_eq!(meshes[0].triangle_count(), 1);
    }

    #[test]
    fn unknown_elements_are_skipped() {
        let ply = b"ply\nformat ascii 1.0\nelement vertex 3\n\
property float x\nproperty float y\nproperty float z\n\
element edge 1\nproperty int vertex1\nproperty int vertex2\n\
element face 1\nproperty list uchar int vertex_indices\nend_header\n\
0 0 0\n1 0 0\n0 1 0\n0 1\n3 0 1 2\n";
        let meshes = load(Path::new("t.ply"), ply).unwrap();
        assert_eq!(meshes[0].triangle_count(), 1);
    }

    #[test]
    fn binary_big_endian() {
        let mut bytes = b"ply\nformat binary_big_endian 1.0\nelement vertex 3\n\
property float x\nproperty float y\nproperty float z\n\
element face 1\nproperty list uchar int vertex_indices\nend_header\n"
            .to_vec();
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for f in p {
                bytes.extend_from_slice(&f.to_be_bytes());
            }
        }
        bytes.push(3u8);
        for i in [0i32, 1, 2] {
            bytes.extend_from_slice(&i.to_be_bytes());
        }
        let meshes = load(Path::new("t.ply"), &bytes).unwrap();
        assert_eq!(meshes[0].triangle_count(), 1);
        assert_eq!(meshes[0].positions[1], Vec3::X);
    }

    #[test]
    fn integer_colors_are_normalized_by_declared_type() {
        let ply = b"ply\nformat ascii 1.0\nelement vertex 3\n\
property float x\nproperty float y\nproperty float z\n\
property uchar red\nproperty uchar green\nproperty uchar blue\n\
element face 1\nproperty list uchar int vertex_indices\nend_header\n\
0 0 0 0 128 0\n1 0 0 0 128 0\n0 1 0 0 128 0\n3 0 1 2\n";
        let meshes = load(Path::new("t.ply"), ply).unwrap();
        let color = meshes[0].original_color.unwrap();
        assert_eq!(color[0], 0.0);
        assert!((color[1] - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(color[2], 0.0);
    }

    #[test]
    fn binary_ply_with_color() {
        let mut bytes = b"ply\nformat binary_little_endian 1.0\nelement vertex 3\n\
property float x\nproperty float y\nproperty float z\n\
property uchar red\nproperty uchar green\nproperty uchar blue\n\
element face 1\nproperty list uchar int vertex_indices\nend_header\n"
            .to_vec();
        for (p, c) in [
            ([0.0f32, 0.0, 0.0], [255u8, 0, 0]),
            ([1.0, 0.0, 0.0], [255, 0, 0]),
            ([0.0, 1.0, 0.0], [255, 0, 0]),
        ] {
            for f in p {
                bytes.extend_from_slice(&f.to_le_bytes());
            }
            bytes.extend_from_slice(&c);
        }
        bytes.push(3u8);
        for i in [0i32, 1, 2] {
            bytes.extend_from_slice(&i.to_le_bytes());
        }
        let meshes = load(Path::new("t.ply"), &bytes).unwrap();
        let m = &meshes[0];
        assert_eq!(m.triangle_count(), 1);
        let c = m.original_color.unwrap();
        assert!((c[0] - 1.0).abs() < 1e-6 && c[1].abs() < 1e-6);
        assert_eq!(m.color, c); // file color is used from the start
    }

    #[test]
    fn truncated_binary_is_error() {
        let ply = b"ply\nformat binary_little_endian 1.0\nelement vertex 3\n\
property float x\nproperty float y\nproperty float z\n\
element face 0\nend_header\n\x00\x00";
        assert!(load(Path::new("t.ply"), ply).is_err());
    }

    #[test]
    fn negative_binary_face_index_is_error() {
        let mut bytes = b"ply\nformat binary_little_endian 1.0\nelement vertex 3\n\
property float x\nproperty float y\nproperty float z\n\
element face 1\nproperty list uchar int vertex_indices\nend_header\n"
            .to_vec();
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for f in p {
                bytes.extend_from_slice(&f.to_le_bytes());
            }
        }
        bytes.push(3);
        for index in [0i32, 1, -1] {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
        assert!(load(Path::new("bad.ply"), &bytes).is_err());
    }

    #[test]
    fn end_header_in_comment_is_not_header_end() {
        let ply = b"ply\nformat ascii 1.0\ncomment not_end_header_here\nelement vertex 3\n\
property float x\nproperty float y\nproperty float z\n\
element face 1\nproperty list uchar int vertex_indices\nend_header\n\
0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n";
        assert_eq!(
            load(Path::new("t.ply"), ply).unwrap()[0].triangle_count(),
            1
        );
    }
}
