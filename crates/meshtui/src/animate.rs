//! Animation / "scene cuts": a YAML file describes a base scene plus a list
//! of cuts, each holding for N frames and changing only what it names
//! (camera, per-mesh color/alpha/visibility, light, wireframe). meshtui
//! renders the frames and assembles them into a GIF (or a PNG strip).
//!
//! Cut semantics — the #1 thing to know:
//! - **Camera values are RELATIVE and accumulate across cuts.** `azimuth: 90`
//!   in two consecutive cuts turns 180° total; `zoom` multiplies (two cuts of
//!   `zoom: 2` = 4×, like the TUI z/Z). Use `azimuth_to` / `elevation_to` for
//!   an ABSOLUTE pose (measured from the base view, ignoring earlier cuts).
//! - **Mesh values are ABSOLUTE** — `color`/`alpha`/`visible`/`scale`/
//!   `translate` set the value. A mesh entry with `path` reloads that mesh's
//!   geometry (per-frame mesh swaps).
//! - `tween: true` on a cut interpolates its changes over its `frames`
//!   (linear; `ease: in|out|inout` to ease) instead of jumping.
//!
//! ```yaml
//! size: [800, 600]               # or "800x600"
//! background: "#1a1b26"          # or transparent: true
//! fps: 12
//! camera: { kind: orthographic, view: "+z" }
//! meshes:
//!   - { path: gear.ply, name: gear, color: "#ff8000" }
//! cuts:
//!   - {}                                    # hold the base scene 1 frame
//!   - frames: 8
//!     camera: { azimuth: 90 }               # +90° from previous
//!   - frames: 12, tween: true, ease: inout
//!     camera: { azimuth_to: 270 }           # glide to an absolute pose
//!   - frames: 4
//!     meshes: [{ name: gear, alpha: 0.3 }]  # fade (absolute)
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
#[serde(default, deny_unknown_fields)]
pub struct Cut {
    /// Frames to hold this state (default: the file's `frames`, else 1).
    pub frames: Option<u32>,
    /// Interpolate this cut's changes over its frames instead of jumping.
    pub tween: Option<bool>,
    /// Easing for a tween: "linear" (default), "in", "out", "inout".
    pub ease: Option<String>,
    pub camera: Option<CameraCut>,
    /// Per-mesh changes, matched by `name` (falls back to index order).
    pub meshes: Vec<MeshEntry>,
    pub light: Option<f32>,
    pub wireframe: Option<f32>,
}

/// Camera changes within a cut (all optional; omitted = keep previous).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct CameraCut {
    pub view: Option<String>,
    /// Relative: added to the current pose (accumulates across cuts).
    pub azimuth: Option<f32>,
    pub elevation: Option<f32>,
    /// Absolute: orbit so azimuth/elevation equal these (measured from the
    /// base view pose), regardless of prior cuts.
    pub azimuth_to: Option<f32>,
    pub elevation_to: Option<f32>,
    pub up: Option<[f32; 3]>,
    /// Relative: multiplies the current zoom (like TUI z/Z).
    pub zoom: Option<f32>,
    /// Absolute: target→camera distance.
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

    let (w, h) = size_override
        .or_else(|| anim.scene.output.size.map(|[a, b]| (a, b)))
        .unwrap_or((800, 600));
    // Fit to the viewport FIRST (the one-time auto-fit resets ortho_scale),
    // then apply the scene's camera/zoom so they aren't cancelled.
    app.set_aspect(w as f32 / h as f32);
    apply_scene_camera(&mut app, &anim.scene);

    let fps = anim.fps.unwrap_or(12).clamp(1, 60);
    let default_frames = anim.frames.unwrap_or(1).max(1);
    let mut frames = Vec::new();

    for (i, cut) in anim.cuts.iter().enumerate() {
        let hold = cut.frames.unwrap_or(default_frames).max(1);
        let tween = cut.tween.unwrap_or(false) && hold > 1;
        // Snapshot state before the cut so a tween can interpolate from it.
        let before = tween.then(|| Snapshot::of(&app));
        for f in 0..hold {
            if tween {
                let t = ease(cut.ease.as_deref(), (f + 1) as f32 / hold as f32);
                apply_cut_tweened(&mut app, before.as_ref().unwrap(), cut, t);
            } else if f == 0 {
                apply_cut(&mut app, cut);
            }
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
        // Absolute pose: reset to the base view, then orbit to az/el.
        if cam.azimuth_to.is_some() || cam.elevation_to.is_some() {
            if let Some(view) = cam
                .view
                .as_deref()
                .and_then(meshtui_core::ViewAxis::parse)
                .or(app.base_view)
            {
                app.camera.set_view_axis(view);
            }
            headless::apply_headless(
                app,
                &HeadlessOpts {
                    azimuth: cam.azimuth_to,
                    elevation: cam.elevation_to,
                    up: cam.up.map(glam::Vec3::from),
                    ..Default::default()
                },
            );
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
        // A cut entry with a `path` (re)loads that mesh: match an existing
        // mesh by name and replace its geometry, else append it. This makes
        // per-frame mesh swaps possible in one animate run.
        if let Some(path) = entry.path.as_deref() {
            let loaded = meshtui_core::loaders::load_path(std::path::Path::new(path))
                .map_err(|e| e.to_string());
            if let Ok(mut meshes) = loaded {
                if let Some(first) = meshes.first_mut() {
                    apply_mesh_cut(first, entry);
                    if let Some(name) = &entry.name {
                        first.name = name.clone();
                    }
                    let idx = entry
                        .name
                        .as_deref()
                        .and_then(|n| app.scene.meshes.iter().position(|m| m.name == n));
                    match idx {
                        Some(i) => {
                            // Keep the replaced mesh's color unless overridden.
                            if entry.color.is_none() {
                                first.color = app.scene.meshes[i].color;
                            }
                            app.scene.meshes[i] = first.clone();
                        }
                        None => app.scene.meshes.push(first.clone()),
                    }
                }
            }
            continue;
        }
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

/// Camera state captured before a tweened cut, to interpolate from.
struct Snapshot {
    orientation: glam::Quat,
    distance: f32,
    ortho_scale: f32,
}

impl Snapshot {
    fn of(app: &App) -> Self {
        Self {
            orientation: app.camera.orientation,
            distance: app.camera.distance,
            ortho_scale: app.camera.ortho_scale,
        }
    }
}

/// Easing curve: linear (default), in, out, inout.
fn ease(name: Option<&str>, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match name.unwrap_or("linear") {
        "in" => t * t,
        "out" => 1.0 - (1.0 - t) * (1.0 - t),
        "inout" => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - 2.0 * (1.0 - t) * (1.0 - t)
            }
        }
        _ => t,
    }
}

/// Interpolate a cut's camera/mesh changes between the pre-cut snapshot and
/// the fully-applied end state at fraction `t` (0..=1).
fn apply_cut_tweened(app: &mut App, before: &Snapshot, cut: &Cut, t: f32) {
    // Snapshot the meshes this cut touches (for color/alpha lerp).
    let touched: Vec<(usize, meshtui_core::Color)> = cut
        .meshes
        .iter()
        .filter_map(|e| {
            let idx = e
                .name
                .as_deref()
                .and_then(|n| app.scene.meshes.iter().position(|m| m.name == n))?;
            Some((idx, app.scene.meshes[idx].color))
        })
        .collect();

    // Camera: reset to the snapshot, then apply the cut's changes scaled by t.
    if let Some(cam) = &cut.camera {
        app.camera.orientation = before.orientation;
        app.camera.distance = before.distance;
        app.camera.ortho_scale = before.ortho_scale;
        let scaled = CameraCut {
            view: cam.view.clone(),
            azimuth: cam.azimuth.map(|a| a * t),
            elevation: cam.elevation.map(|e| e * t),
            azimuth_to: cam.azimuth_to.map(|a| a * t),
            elevation_to: cam.elevation_to.map(|e| e * t),
            up: cam.up,
            zoom: cam.zoom.map(|z| 1.0 + (z - 1.0) * t),
            distance: cam
                .distance
                .map(|d| before.distance + (d - before.distance) * t),
            kind: cam.kind.clone(),
            fov: cam.fov,
        };
        let c = cut.clone_with_camera(scaled);
        apply_cut(app, &c);
    } else {
        let c = cut.clone_no_camera();
        apply_cut(app, &c);
    }

    // Mesh color/alpha: apply_cut set the absolute end value; lerp from the
    // snapshot toward it by t so fades/tints are smooth.
    for (idx, start) in touched {
        if let Some(entry) = cut
            .meshes
            .iter()
            .find(|e| e.name.as_deref() == Some(app.scene.meshes[idx].name.as_str()))
        {
            if entry.color.is_some() || entry.alpha.is_some() {
                let end = app.scene.meshes[idx].color;
                let mut c = [0.0; 4];
                for ch in 0..4 {
                    c[ch] = start[ch] + (end[ch] - start[ch]) * t;
                }
                app.scene.meshes[idx].color = c;
            }
        }
    }
}

impl Cut {
    fn clone_with_camera(&self, camera: CameraCut) -> Cut {
        Cut {
            camera: Some(camera),
            ..self.clone_no_camera()
        }
    }
    fn clone_no_camera(&self) -> Cut {
        Cut {
            frames: self.frames,
            tween: self.tween,
            ease: self.ease.clone(),
            camera: None,
            meshes: self.meshes.clone(),
            light: self.light,
            wireframe: self.wireframe,
        }
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
///
/// GIF has 1-bit alpha, so transparency is binary: pixels with alpha 0 map to
/// the transparent palette index, anything above renders opaque (composited
/// over the background). The gif quantizer never assigns the transparent
/// index on its own, so transparent frames are built by hand. A dissolve-style
/// fade needs the PNG frames + ffmpeg (palettegen=reserve_transparent with
/// dithered alpha).
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
        let mut frame = if anim.transparent {
            // Quantize the frame as opaque RGB, then move alpha-0 pixels into a
            // dedicated transparent palette slot (0) so the background shows
            // through instead of rendering black.
            let mut rgb: Vec<u8> = Vec::with_capacity(pixels.len() / 4 * 3);
            for px in pixels.as_chunks::<4>().0 {
                rgb.extend_from_slice(&px[..3]);
            }
            let mut f = Frame::from_rgb_speed(anim.width as u16, anim.height as u16, &rgb, 10);
            // Prepend a transparent slot; shift existing palette indices by 1.
            let existing = f.palette.take().unwrap_or_default();
            let mut palette = vec![0u8, 0, 0];
            palette.extend_from_slice(&existing);
            f.palette = Some(palette);
            let mut buffer = f.buffer.into_owned();
            for (i, px) in pixels.as_chunks::<4>().0.iter().enumerate() {
                buffer[i] = if px[3] == 0 {
                    0
                } else {
                    buffer[i].saturating_add(1)
                };
            }
            f.buffer = std::borrow::Cow::Owned(buffer);
            f.transparent = Some(0);
            f
        } else {
            let mut rgb: Vec<u8> = Vec::with_capacity(pixels.len() / 4 * 3);
            for px in pixels.as_chunks::<4>().0 {
                rgb.extend_from_slice(&px[..3]);
            }
            Frame::from_rgb_speed(anim.width as u16, anim.height as u16, &rgb, 10)
        };
        frame.delay = delay_cs;
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
        app.base_view = Some(view);
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

    #[test]
    fn cut_parses_tween_ease_and_absolute_camera() {
        let anim = parse_str(
            "meshes: [{path: a.ply}]\ncuts:\n  - frames: 10\n    tween: true\n    ease: inout\n    camera: { azimuth_to: 270, zoom: 2 }\n",
        )
        .unwrap();
        let cut = &anim.cuts[0];
        assert_eq!(cut.tween, Some(true));
        assert_eq!(cut.ease.as_deref(), Some("inout"));
        let cam = cut.camera.as_ref().unwrap();
        assert_eq!(cam.azimuth_to, Some(270.0));
        assert_eq!(cam.zoom, Some(2.0));
    }

    #[test]
    fn ease_curves_endpoints_and_shape() {
        assert_eq!(ease(None, 0.0), 0.0);
        assert_eq!(ease(None, 1.0), 1.0);
        assert!((ease(Some("in"), 0.5) - 0.25).abs() < 1e-6);
        assert!((ease(Some("out"), 0.5) - 0.75).abs() < 1e-6);
        assert!((ease(Some("inout"), 0.5) - 0.5).abs() < 1e-6);
    }
}
