//! Benchmark: software rasterizer, 160k triangles at 800×600.

use criterion::{criterion_group, criterion_main, Criterion};
use glam::Vec3;
use meshtui_core::{Camera, CameraKind, Mesh, Scene};
use meshtui_render::software::{Lighting, Options, SoftwareRasterizer};
use meshtui_render::RenderBackend;

/// UV-sphere-ish grid mesh with ~2*lat*lon triangles.
fn sphere_mesh(lat: usize, lon: usize) -> Mesh {
    let mut m = Mesh::new("sphere");
    for i in 0..=lat {
        let phi = std::f32::consts::PI * i as f32 / lat as f32;
        for j in 0..lon {
            let theta = 2.0 * std::f32::consts::PI * j as f32 / lon as f32;
            let p = Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin());
            m.positions.push(p);
            m.normals.push(p);
        }
    }
    for i in 0..lat {
        for j in 0..lon {
            let a = (i * lon + j) as u32;
            let b = (i * lon + (j + 1) % lon) as u32;
            let c = ((i + 1) * lon + j) as u32;
            let d = ((i + 1) * lon + (j + 1) % lon) as u32;
            m.indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    m
}

fn bench_raster(c: &mut Criterion) {
    // ~160k triangles
    let mesh = sphere_mesh(200, 400);
    let mut scene = Scene::new();
    scene.meshes.push(mesh);
    let (min, max) = scene.visible_bounds().unwrap();
    let cam = Camera::frame_bounds(min, max, CameraKind::Perspective, 60.0, 1.1);

    let opts = Options {
        lighting: Lighting {
            key_intensity: 1.5,
            fill_intensity: 1.5,
            fill_dir: Vec3::new(-0.5, 0.3, -1.0).normalize(),
            rim_intensity: 0.6,
            rim_dir: Vec3::new(0.5, 0.2, 1.0).normalize(),
            ambient: [0.15, 0.15, 0.15],
        },
        shininess: 30.0,
        wireframe_thickness: 0.0,
        wireframe_color: [60, 60, 60, 255],
    };
    let mut r = SoftwareRasterizer { options: opts };

    c.bench_function("raster_160k_tris_800x600", |b| {
        b.iter(|| r.render(&scene, &cam, 800, 600))
    });
}

criterion_group!(benches, bench_raster);
criterion_main!(benches);
