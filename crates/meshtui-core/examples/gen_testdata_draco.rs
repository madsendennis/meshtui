//! Generates the Draco-compressed test meshes into `testdata/meshes/`:
//! `cube.drc`, `cube_colored.drc` (normals + colors) and `cube_draco.glb`
//! (KHR_draco_mesh_compression primitive). Run from the workspace root:
//!
//! ```sh
//! cargo run -p meshtui-core --example gen_testdata_draco
//! ```

use std::path::PathBuf;

use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{encode_mesh, Config};
use draco_oxide::{AttributeDomain, AttributeType, MeshBuilder, NdVector};

/// Unit cube: 8 corners, 12 triangles (shared with testdata/generate.py).
fn cube() -> (Vec<[f32; 3]>, Vec<[usize; 3]>) {
    let positions: Vec<[f32; 3]> = [-0.5f32, 0.5]
        .iter()
        .flat_map(|&x| {
            [-0.5, 0.5]
                .iter()
                .flat_map(move |&y| [-0.5, 0.5].iter().map(move |&z| [x, y, z]))
        })
        .collect();
    let quads = [
        [0, 1, 3, 2],
        [4, 6, 7, 5], // z- / z+
        [0, 4, 5, 1],
        [2, 3, 7, 6], // y- / y+
        [0, 2, 6, 4],
        [1, 5, 7, 3], // x- / x+
    ];
    let mut tris = Vec::new();
    for q in quads {
        tris.push([q[0], q[1], q[2]]);
        tris.push([q[0], q[2], q[3]]);
    }
    (positions, tris)
}

fn vertex_normals(positions: &[[f32; 3]], tris: &[[usize; 3]]) -> Vec<[f32; 3]> {
    let mut acc = vec![[0.0f32; 3]; positions.len()];
    for t in tris {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| positions[i]);
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        for &i in t {
            for k in 0..3 {
                acc[i][k] += n[k];
            }
        }
    }
    for a in &mut acc {
        let len = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-9);
        *a = [a[0] / len, a[1] / len, a[2] / len];
    }
    acc
}

/// Red-to-blue gradient by height, matching generate.py's rainbow_colors.
fn gradient_colors(positions: &[[f32; 3]]) -> Vec<[f32; 3]> {
    positions
        .iter()
        .map(|p| {
            let t = p[1] + 0.5;
            [t, 0.4, 1.0 - t]
        })
        .collect()
}

/// Builds and encodes a draco mesh; attribute ids are assigned in add order.
fn encode(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    colors: Option<&[[f32; 3]]>,
    tris: &[[usize; 3]],
) -> Vec<u8> {
    let mut builder = MeshBuilder::new();
    let pos: Vec<NdVector<3, f32>> = positions.iter().map(|&p| p.into()).collect();
    let pos_id = builder.add_attribute(
        pos,
        AttributeType::Position,
        AttributeDomain::Position,
        vec![],
    );
    let nrm: Vec<NdVector<3, f32>> = normals.iter().map(|&n| n.into()).collect();
    builder.add_attribute(
        nrm,
        AttributeType::Normal,
        AttributeDomain::Position,
        vec![pos_id],
    );
    if let Some(colors) = colors {
        let col: Vec<NdVector<3, f32>> = colors.iter().map(|&c| c.into()).collect();
        builder.add_attribute(
            col,
            AttributeType::Color,
            AttributeDomain::Position,
            vec![pos_id],
        );
    }
    builder.set_connectivity_attribute(tris.to_vec());
    let mesh = builder.build().expect("cube mesh should be valid");
    let mut bytes = Vec::new();
    encode_mesh(mesh, &mut bytes, Config::default()).expect("cube should encode");
    bytes
}

/// Wraps an encoded draco stream in a GLB with a KHR_draco_mesh_compression
/// primitive. Attribute ids match the add order in `encode` (0=pos, 1=nrm).
fn draco_glb(draco_bytes: &[u8]) -> Vec<u8> {
    let json = serde_json::json!({
        "asset": {"version": "2.0", "generator": "meshtui testdata"},
        "extensionsUsed": ["KHR_draco_mesh_compression"],
        "extensionsRequired": ["KHR_draco_mesh_compression"],
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{
            "name": "draco_cube",
            "primitives": [{
                "mode": 4,
                "attributes": {"POSITION": 0},
                "extensions": {
                    "KHR_draco_mesh_compression": {
                        "bufferView": 0,
                        "attributes": {"POSITION": 0, "NORMAL": 1}
                    }
                }
            }]
        }],
        "buffers": [{"byteLength": draco_bytes.len()}],
        "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": draco_bytes.len()}]
    });
    let mut js = serde_json::to_vec(&json).unwrap();
    while !js.len().is_multiple_of(4) {
        js.push(b' ');
    }
    let mut bin = draco_bytes.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + js.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&0x46546C67u32.to_le_bytes()); // "glTF"
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(js.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F534Au32.to_le_bytes()); // "JSON"
    out.extend_from_slice(&js);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E4942u32.to_le_bytes()); // "BIN\0"
    out.extend_from_slice(&bin);
    out
}

fn main() {
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/meshes");
    let out_dir = out_dir
        .canonicalize()
        .expect("testdata/meshes must exist; run testdata/generate.py first");
    let (positions, tris) = cube();
    let normals = vertex_normals(&positions, &tris);
    let colors = gradient_colors(&positions);

    let plain = encode(&positions, &normals, None, &tris);
    std::fs::write(out_dir.join("cube.drc"), &plain).unwrap();
    println!("wrote cube.drc ({} bytes)", plain.len());

    let colored = encode(&positions, &normals, Some(&colors), &tris);
    std::fs::write(out_dir.join("cube_colored.drc"), &colored).unwrap();
    println!("wrote cube_colored.drc ({} bytes)", colored.len());

    let glb = draco_glb(&plain);
    std::fs::write(out_dir.join("cube_draco.glb"), &glb).unwrap();
    println!("wrote cube_draco.glb ({} bytes)", glb.len());
}
