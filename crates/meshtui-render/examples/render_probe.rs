//! Headless render probe: renders a mesh file OR directory from a given orbit
//! angle to a PPM, replicating the app's lighting setup.

use std::path::{Path, PathBuf};

use meshtui_core::{Camera, Scene};
use meshtui_render::software::{Lighting, Options, SoftwareRasterizer};
use meshtui_render::RenderBackend;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "tmp/16.ply".to_string());
    let theta_deg: f32 = args.next().map(|s| s.parse().unwrap()).unwrap_or(0.0);
    let phi_deg: f32 = args.next().map(|s| s.parse().unwrap()).unwrap_or(90.0);
    let out = args
        .next()
        .unwrap_or_else(|| "/tmp/render_probe.ppm".to_string());

    let mut paths: Vec<PathBuf> = Vec::new();
    let p = Path::new(&path);
    if p.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(p)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|e| {
                matches!(
                    e.extension().and_then(|x| x.to_str()),
                    Some("stl" | "obj" | "ply")
                )
            })
            .collect();
        entries.sort();
        paths.extend(entries);
    } else {
        paths.push(p.to_path_buf());
    }

    let mut scene = Scene::new();
    for path in &paths {
        for mut m in meshtui_core::loaders::load_meshes(path).expect("load mesh") {
            if m.original_color.is_none() {
                m.color = [0.85, 0.85, 0.87, 1.0];
            }
            scene.meshes.push(m);
        }
    }
    let (bmin, bmax) = scene.visible_bounds().expect("bounds");
    let mut camera = Camera::frame_bounds(
        bmin,
        bmax,
        meshtui_core::CameraKind::Orthographic,
        60.0,
        1.0,
    );
    camera.theta = theta_deg.to_radians();
    camera.phi = phi_deg.to_radians();

    let mut rast = SoftwareRasterizer {
        options: Options {
            lighting: Lighting {
                key_intensity: 1.5,
                fill_intensity: 1.5,
                fill_dir: glam::Vec3::ZERO,
                rim_intensity: 0.6,
                rim_dir: glam::Vec3::ZERO,
                ambient: [0.15, 0.15, 0.15],
            },
            shininess: 50.0,
            wireframe_thickness: 0.0,
            wireframe_color: [0, 0, 0, 255],
        },
    };

    // Same call the app makes in update_light_dirs() (config defaults).
    let fwd = (camera.position() - camera.target).normalize();
    rast.options.lighting.fill_dir =
        meshtui_render::software::camera_light_offset(fwd, camera.up, -45.0, 15.0);
    rast.options.lighting.rim_dir =
        meshtui_render::software::camera_light_offset(fwd, camera.up, 135.0, 10.0);

    let frame = rast
        .render(&scene, &camera, 1200, 900)
        .expect("render frame");
    let mut ppm = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
    for px in frame.pixels.as_chunks::<4>().0 {
        ppm.extend_from_slice(&px[..3]);
    }
    std::fs::write(&out, ppm).expect("write ppm");
    eprintln!(
        "eye={:?} fill={:?} rim={:?}",
        camera.position(),
        rast.options.lighting.fill_dir,
        rast.options.lighting.rim_dir
    );
}
