//! Animation / "scene cuts": a YAML file describes a base scene plus a list
//! of cuts, each holding for N frames and changing only what it names
//! (camera, per-mesh color/alpha/visibility, light, wireframe). meshtui
//! renders the frames and assembles them into a GIF (or a PNG strip).
//!
//! ```yaml
//! # Top level reuses the scene-file format (meshes, camera, light, output).
//! size: [800, 600]
//! background: "#1a1b26"        # or transparent: true
//! fps: 12
//! camera: { kind: orthographic, view: "+z" }
//! meshes:
//!   - { path: gear.ply, name: gear, color: "#ff8000" }
//!   - { path: base.ply, name: base, color: gray }
//! cuts:
//!   - {}                        # hold the base scene
//!   - frames: 8                 # camera-only change
//!     camera: { azimuth: 90 }
//!   - frames: 8
//!     meshes: [{ name: gear, color: red }]   # recolor one mesh
//! ```

use std::path::Path;

use serde::Deserialize;

use crate::app::App;
use crate::headless::{self, HeadlessOpts};
use meshtui_core::config::Config;
use meshtui_core::scene_file::{MeshEntry, SceneFile};
use meshtui_core::{Color, Scene};

/// An animation file: a base scene plus the frame cadence and cuts.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AnimFile {
    /// Reused scene-file fields (meshes, camera, output, light, wireframe).
    #[serde(flatten)]
    pub scene: SceneFile,
    /// Frames per second in the output GIF.
    pub fps: Option<u32>,
    /// Default hold (frames) for a cut that omits `frames`.
    pub frames: Option<u32>,
    pub cuts: Vec<Cut>,
}

/// One cut: hold for `frames`, applying only the named changes.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Cut {
    /// Frames to hold this state (default: the file's `frames`, else 1).
    pub frames: Option<u32>,
    pub camera: Option<CameraCut>,
    /// Per-mesh changes, matched by `name` (falls back to index order).
    pub meshes: Vec<MeshEntry>,
    pub light: Option<f32>,
    pub wireframe: Option<f32>,
}

/// Camera changes within a cut (all optional; omitted = keep previous).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CameraCut {
    pub view: Option<String>,
    pub azimuth: Option<f32>,
    pub elevation: Option<f32>,
    pub up: Option<[f32; 3]>,
    pub zoom: Option<f32>,
    pub distance: Option<f32>,
    pub kind: Option<String>,
    pub fov: Option<f32>,
}

/// Parse an animation file from YAML text.
pub fn parse_str(text: &str) -> Result<AnimFile, String> {
    let mut anim: AnimFile = serde_yml::from_str(text).map_err(|e| e.to_string())?;
    // The flattened SceneFile doesn't fold its flat output keys when embedded
    // via #[serde(flatten)], so do it here.
    anim.scene.fold_output();
    Ok(anim)
}

/// Load an animation file from disk.
pub fn load(path: &Path) -> Result<AnimFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse_str(&text)
}

/// Rendered animation ready to encode.
pub struct RenderedAnimation {
    pub frames: Vec<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub background: Option<Color>,
    pub transparent: bool,
}

/// Render every frame of an animation. Camera/mesh state persists across
/// cuts (each cut only changes what it names).
pub fn render(
    anim: &AnimFile,
    config_path: Option<&Path>,
    size_override: Option<(u32, u32)>,
) -> Result<RenderedAnimation, String> {
    let scene: Scene = anim.scene.build_scene().map_err(|e| e.to_string())?;
    let config = Config::load_effective(config_path).map_err(|e| e.to_string())?;
    let mut app = App::new(scene, config);
    apply_scene_camera(&mut app, &anim.scene);

    let (w, h) = size_override
        .or_else(|| anim.scene.output.size.map(|[a, b]| (a, b)))
        .unwrap_or((800, 600));
    app.set_aspect(w as f32 / h as f32);

    let fps = anim.fps.unwrap_or(12).clamp(1, 60);
    let default_frames = anim.frames.unwrap_or(1).max(1);
    let mut frames = Vec::new();

    for (i, cut) in anim.cuts.iter().enumerate() {
        apply_cut(&mut app, cut);
        let hold = cut.frames.unwrap_or(default_frames).max(1);
        for _ in 0..hold {
            let frame = app
                .render_frame(w, h)
                .ok_or_else(|| format!("cut {i}: nothing to render (all meshes hidden)"))?;
            frames.push(frame.pixels);
        }
    }
    if frames.is_empty() {
        return Err("animation has no frames (empty cuts)".into());
    }
    let transparent = anim.scene.output.transparent.unwrap_or(false);
    Ok(RenderedAnimation {
        frames,
        width: w,
        height: h,
        fps,
        background: if transparent {
            None
        } else {
            anim.scene.output.background.as_ref().map(|c| c.0)
        },
        transparent,
    })
}

/// Apply a cut's changes to the app (state persists between cuts).
fn apply_cut(app: &mut App, cut: &Cut) {
    if let Some(cam) = &cut.camera {
        if let Some(kind) = cam.kind.as_deref() {
            app.camera.kind = match kind {
                "perspective" | "persp" => meshtui_core::CameraKind::Perspective,
                _ => meshtui_core::CameraKind::Orthographic,
            };
        }
        if let Some(fov) = cam.fov {
            app.camera.fov_degrees = fov;
        }
        if let Some(up) = cam.up {
            app.camera.set_up(glam::Vec3::from(up));
        }
        if let Some(view) = cam.view.as_deref().and_then(meshtui_core::ViewAxis::parse) {
            app.camera.set_view_axis(view);
        }
        if cam.azimuth.is_some() || cam.elevation.is_some() {
            headless::apply_headless(
                app,
                &HeadlessOpts {
                    azimuth: cam.azimuth,
                    elevation: cam.elevation,
                    ..Default::default()
                },
            );
        }
        if let Some(zoom) = cam.zoom {
            app.camera.zoom(zoom);
        }
        if let Some(distance) = cam.distance {
            app.camera.distance = distance.max(1e-3);
        }
    }
    for entry in &cut.meshes {
        // Match by name first, else by index among current meshes.
        let idx = entry
            .name
            .as_deref()
            .and_then(|n| app.scene.meshes.iter().position(|m| m.name == n))
            .or_else(|| entry.name.as_deref()?.parse::<usize>().ok());
        if let Some(i) = idx {
            if let Some(mesh) = app.scene.meshes.get_mut(i) {
                apply_mesh_cut(mesh, entry);
            }
        }
    }
    if let Some(light) = cut.light {
        app.set_light_scale(light);
    }
    if let Some(wireframe) = cut.wireframe {
        app.set_wireframe_thickness(wireframe);
    }
}

/// Apply a mesh cut's fields (only the ones present) to a live mesh.
fn apply_mesh_cut(mesh: &mut meshtui_core::Mesh, entry: &MeshEntry) {
    if let Some(color) = &entry.color {
        mesh.color = color.0;
    }
    if let Some(alpha) = entry.alpha {
        mesh.color[3] = alpha.clamp(0.0, 1.0);
    }
    if let Some(visible) = entry.visible {
        mesh.visible = visible;
    }
    if entry.scale.is_some() || entry.translate.is_some() {
        let scale = entry.scale.unwrap_or(1.0);
        let t = entry
            .translate
            .map(glam::Vec3::from)
            .unwrap_or(glam::Vec3::ZERO);
        for p in &mut mesh.positions {
            *p = *p * scale + t;
        }
        mesh.compute_normals();
    }
}

/// Composite a raw frame over the background (no-op when transparent).
fn composite_frame(mut pixels: Vec<u8>, background: Option<Color>) -> Vec<u8> {
    let Some(bg) = background else { return pixels };
    let (r, g, b) = (bg[0], bg[1], bg[2]);
    for px in pixels.as_chunks_mut::<4>().0.iter_mut() {
        let a = px[3] as f32 / 255.0;
        if a < 1.0 {
            px[0] = (px[0] as f32 / 255.0 * a + r * (1.0 - a))
                .round()
                .clamp(0.0, 255.0) as u8;
            px[1] = (px[1] as f32 / 255.0 * a + g * (1.0 - a))
                .round()
                .clamp(0.0, 255.0) as u8;
            px[2] = (px[2] as f32 / 255.0 * a + b * (1.0 - a))
                .round()
                .clamp(0.0, 255.0) as u8;
            px[3] = 255;
        }
    }
    pixels
}

/// Encode the rendered frames as an animated GIF.
pub fn encode_gif(anim: &RenderedAnimation, out: &Path) -> Result<(), String> {
    use gif::{Encoder, Frame, Repeat};
    let mut file = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let mut encoder = Encoder::new(&mut file, anim.width as u16, anim.height as u16, &[])
        .map_err(|e| e.to_string())?;
    encoder
        .set_repeat(Repeat::Infinite)
        .map_err(|e| e.to_string())?;
    let delay_cs = (100 / anim.fps.max(1)) as u16; // hundredths of a second

    for raw in &anim.frames {
        let pixels = composite_frame(raw.clone(), anim.background);
        // Quantize RGBA→indexed. For transparent output, reserve index 0.
        let mut frame = if anim.transparent {
            Frame::from_rgba_speed(
                anim.width as u16,
                anim.height as u16,
                &mut pixels.clone(),
                10,
            )
        } else {
            let mut rgb: Vec<u8> = Vec::with_capacity(pixels.len() / 4 * 3);
            for px in pixels.as_chunks::<4>().0 {
                rgb.extend_from_slice(&px[..3]);
            }
            Frame::from_rgb_speed(anim.width as u16, anim.height as u16, &rgb, 10)
        };
        frame.delay = delay_cs;
        if anim.transparent {
            frame.transparent = Some(0);
        }
        encoder.write_frame(&frame).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Reuse the scene-file camera application (kept in sync with main.rs).
fn apply_scene_camera(app: &mut App, file: &SceneFile) {
    let cam = &file.camera;
    if let Some(kind) = file.camera_kind() {
        app.camera.kind = kind;
    }
    if let Some(fov) = cam.fov {
        app.camera.fov_degrees = fov;
    }
    if let Some(up) = cam.up {
        app.camera.set_up(glam::Vec3::from(up));
    }
    if let Some(view) = cam.view.as_deref().and_then(meshtui_core::ViewAxis::parse) {
        app.camera.set_view_axis(view);
    }
    if cam.azimuth.is_some() || cam.elevation.is_some() {
        headless::apply_headless(
            app,
            &HeadlessOpts {
                azimuth: cam.azimuth,
                elevation: cam.elevation,
                ..Default::default()
            },
        );
    }
    if let Some(zoom) = cam.zoom {
        app.camera.zoom(zoom);
    }
    if let Some(distance) = cam.distance {
        app.camera.distance = distance.max(1e-3);
    }
    if let Some(light) = file.light {
        app.set_light_scale(light);
    }
    if let Some(wireframe) = file.wireframe {
        app.set_wireframe_thickness(wireframe);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_animation_with_cuts() {
        let yaml = r##"
size: [320, 240]
fps: 10
camera: { kind: orthographic, view: "+z" }
meshes:
  - { path: a.ply, name: gear, color: "#ff8000" }
cuts:
  - {}
  - frames: 8
    camera: { azimuth: 90 }
  - frames: 4
    meshes: [{ name: gear, color: red, alpha: 0.5 }]
    light: 2.0
"##;
        let anim = parse_str(yaml).unwrap();
        assert_eq!(anim.fps, Some(10));
        assert_eq!(anim.scene.output.size, Some([320, 240]));
        assert_eq!(anim.cuts.len(), 3);
        assert_eq!(anim.cuts[1].frames, Some(8));
        assert_eq!(anim.cuts[1].camera.as_ref().unwrap().azimuth, Some(90.0));
        let mesh_cut = &anim.cuts[2].meshes[0];
        assert_eq!(mesh_cut.name.as_deref(), Some("gear"));
        assert_eq!(mesh_cut.alpha, Some(0.5));
        assert_eq!(anim.cuts[2].light, Some(2.0));
    }

    #[test]
    fn empty_cuts_parse() {
        let anim = parse_str("meshes: [{path: a.ply}]\n").unwrap();
        assert!(anim.cuts.is_empty());
    }
}
