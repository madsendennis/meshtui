//! Benchmark: 1M-triangle binary STL parse (target <100ms).

use criterion::{criterion_group, criterion_main, Criterion};

/// Build an in-memory binary STL with `n` triangles.
fn binary_stl(n: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; 80];
    bytes.extend_from_slice(&(n as u32).to_le_bytes());
    bytes.reserve(n * 50);
    for i in 0..n {
        let f = i as f32;
        // normal + 3 verts + attr
        for v in [0.0, 0.0, 1.0, f, 0.0, 0.0, f + 1.0, 0.0, 0.0, f, 1.0, 0.0] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
    }
    bytes
}

fn bench_stl_parse(c: &mut Criterion) {
    let bytes = binary_stl(1_000_000);
    c.bench_function("parse_1m_tri_stl", |b| {
        b.iter(|| {
            let dir = std::env::temp_dir().join("meshtui_bench.stl");
            std::fs::write(&dir, &bytes).unwrap();
            let meshes = meshtui_core::loaders::load_meshes(&dir).unwrap();
            std::hint::black_box(meshes[0].triangle_count())
        })
    });
}

criterion_group!(benches, bench_stl_parse);
criterion_main!(benches);
