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
//!   (linear; `ease: in|out|inout` to ease) instead of jumping. Every tween
//!   frame derives from the pre-cut state, so values never compound.
//! - Unknown top-level keys and unknown `ease` names are hard errors.
//!
//! GIF output note: GIF has 1-bit (binary) alpha. With `transparent: true`,
//! fully transparent and opaque pixels are preserved, while intermediate alpha
//! uses deterministic ordered Bayer dithering to approximate partial coverage.
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

/// Top-level keys an animation file accepts (the SceneFile keys plus the
/// animation cadence). `#[serde(flatten)]` silently disables
/// deny_unknown_fields, so the check is done by hand.
const ANIM_TOP_LEVEL_KEYS: &[&str] = &[
    "meshes",
    "camera",
    "output",
    "light",
    "wireframe",
    "size",
    "background",
    "transparent",
    "fps",
    "frames",
    "cuts",
];

/// Parse an animation file from YAML text.
pub fn parse_str(text: &str) -> Result<AnimFile, String> {
    let value: serde_yml::Value = serde_yml::from_str(text).map_err(|e| e.to_string())?;
    if let Some(mapping) = value.as_mapping() {
        for key in mapping.keys().map(|k| k.as_str()) {
            if !ANIM_TOP_LEVEL_KEYS.contains(&key) {
                return Err(format!(
                    "unknown animation key {key:?} (expected one of: {})",
                    ANIM_TOP_LEVEL_KEYS.join(", ")
                ));
            }
        }
    }
    let mut anim: AnimFile = serde_yml::from_value(value).map_err(|e| e.to_string())?;
    // The flattened SceneFile doesn't fold its flat output keys when embedded
    // via #[serde(flatten)], so do it here.
    anim.scene.fold_output();
    for (i, cut) in anim.cuts.iter().enumerate() {
        if let Some(ease) = cut.ease.as_deref() {
            if !matches!(ease, "linear" | "in" | "out" | "inout") {
                return Err(format!(
                    "cut {i}: unknown ease {ease:?} (linear|in|out|inout)"
                ));
            }
        }
    }
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
    // Pose first (kind/fov/up/view/azimuth/elevation — the fit must use the
    // final projection), then the one-time auto-fit (which sets
    // distance/ortho_scale from that pose), then zoom/distance so the fit
    // doesn't cancel them.
    crate::apply_scene_camera_pose(&mut app, &anim.scene);
    app.set_aspect(w as f32 / h as f32);
    crate::apply_scene_camera_post(&mut app, &anim.scene);

    let fps = anim.fps.unwrap_or(12).clamp(1, 60);
    let default_frames = anim.frames.unwrap_or(1).max(1);
    let mut frames = Vec::new();
    let mut state = AnimState::of(&app.scene);

    for (i, cut) in anim.cuts.iter().enumerate() {
        let hold = cut.frames.unwrap_or(default_frames).max(1);
        let tween = cut.tween.unwrap_or(false) && hold > 1;
        // Snapshot state before the cut so a tween can interpolate from it.
        let before = tween.then(|| Snapshot::of(&app));
        for f in 0..hold {
            if tween {
                let t = ease(cut.ease.as_deref(), (f + 1) as f32 / hold as f32);
                apply_cut_tweened(
                    &mut app,
                    before.as_ref().unwrap(),
                    cut,
                    t,
                    &mut state,
                    f == 0,
                )
                .map_err(|e| format!("cut {i}: {e}"))?;
            } else if f == 0 {
                apply_cut(&mut app, cut, &mut state).map_err(|e| format!("cut {i}: {e}"))?;
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

/// Per-mesh base geometry for ABSOLUTE cut transforms: scale/translate are
/// authored relative to the loaded mesh, never to the previous cut, so
/// repeating `scale: 2` stays 2× instead of compounding.
struct AnimState {
    base_positions: Vec<Vec<glam::Vec3>>,
}

impl AnimState {
    fn of(scene: &Scene) -> Self {
        Self {
            base_positions: scene.meshes.iter().map(|m| m.positions.clone()).collect(),
        }
    }
}

/// The world up axis for a cut's orbits: an explicit `up` wins, else the
/// persistent scene up (never the camera's mutable, possibly tilted up).
fn cut_world_up(app: &App, cam: &CameraCut) -> glam::Vec3 {
    cam.up.map(glam::Vec3::from).unwrap_or(app.base_up)
}

/// Apply a cut's changes to the app (state persists between cuts).
fn apply_cut(app: &mut App, cut: &Cut, state: &mut AnimState) -> Result<(), String> {
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
            let up = glam::Vec3::from(up);
            app.camera.set_up(up);
            app.base_up = up.normalize_or(glam::Vec3::Y);
        }
        if let Some(view) = cam.view.as_deref().and_then(meshtui_core::ViewAxis::parse) {
            app.camera.set_view_axis(view);
        }
        // Absolute pose: reset to the base pose captured after scene-camera
        // setup (a view in the same cut overrides it), then orbit to az/el.
        if cam.azimuth_to.is_some() || cam.elevation_to.is_some() {
            if cam.view.is_none() {
                app.camera.orientation = app.base_orientation;
            }
            let up = cut_world_up(app, cam);
            app.camera.orbit_around(
                up,
                cam.azimuth_to.unwrap_or(0.0).to_radians(),
                cam.elevation_to.unwrap_or(0.0).to_radians(),
            );
        }
        if cam.azimuth.is_some() || cam.elevation.is_some() {
            let up = cut_world_up(app, cam);
            app.camera.orbit_around(
                up,
                cam.azimuth.unwrap_or(0.0).to_radians(),
                cam.elevation.unwrap_or(0.0).to_radians(),
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
            reload_mesh(app, state, entry, path)?;
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
                apply_mesh_cut(mesh, entry, state.base_positions.get(i).map(Vec::as_slice));
            }
        }
    }
    if let Some(light) = cut.light {
        app.set_light_scale(light);
    }
    if let Some(wireframe) = cut.wireframe {
        app.set_wireframe_thickness(wireframe);
    }
    Ok(())
}

/// (Re)load a mesh named by a cut entry, replacing the same-named mesh (or
/// appending a new one). Fields the cut omits survive the reload — notably a
/// cut of `{name, path, alpha}` keeps the old color AND applies the alpha.
fn reload_mesh(
    app: &mut App,
    state: &mut AnimState,
    entry: &MeshEntry,
    path: &str,
) -> Result<(), String> {
    let mut meshes = meshtui_core::loaders::load_path(std::path::Path::new(path))
        .map_err(|e| format!("cannot load mesh {path:?}: {e}"))?;
    if meshes.is_empty() {
        return Err(format!("no geometry in {path:?}"));
    }
    let mut loaded = meshes.swap_remove(0);
    let idx = entry
        .name
        .as_deref()
        .and_then(|n| app.scene.meshes.iter().position(|m| m.name == n));
    // Preserve the replaced mesh's fields the cut doesn't name. Color is
    // restored BEFORE the entry's own color/alpha so an explicit alpha on a
    // color-less entry isn't clobbered by the old RGBA.
    if entry.color.is_none() {
        if let Some(old) = idx.map(|i| &app.scene.meshes[i]) {
            loaded.color = old.color;
        }
    }
    if let Some(color) = &entry.color {
        loaded.color = color.0;
    }
    if let Some(alpha) = entry.alpha {
        loaded.color[3] = alpha.clamp(0.0, 1.0);
    }
    if let Some(visible) = entry.visible {
        loaded.visible = visible;
    }
    if let Some(name) = &entry.name {
        loaded.name = name.clone();
    }
    // The freshly loaded geometry becomes the base for absolute transforms.
    let base = loaded.positions.clone();
    apply_transform(&mut loaded, entry, Some(&base));
    match idx {
        Some(i) => {
            app.scene.meshes[i] = loaded;
            state.base_positions[i] = base;
        }
        None => {
            app.scene.meshes.push(loaded);
            state.base_positions.push(base);
        }
    }
    Ok(())
}

/// Scene state captured before a tweened cut, to interpolate from. Mesh
/// colors are captured for every mesh (cuts touch by name), and light/
/// wireframe ride along so their tweens also start from the pre-cut values.
struct Snapshot {
    orientation: glam::Quat,
    distance: f32,
    ortho_scale: f32,
    mesh_colors: Vec<meshtui_core::Color>,
    light: f32,
    wireframe: f32,
}

impl Snapshot {
    fn of(app: &App) -> Self {
        Self {
            orientation: app.camera.orientation,
            distance: app.camera.distance,
            ortho_scale: app.camera.ortho_scale,
            mesh_colors: app.scene.meshes.iter().map(|m| m.color).collect(),
            light: app.light_scale(),
            wireframe: app.wireframe_thickness(),
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

/// Interpolate a cut's changes between the pre-cut snapshot and the cut's
/// absolute end state at fraction `t` (0..=1). Every frame derives from the
/// snapshot and the meshes' base geometry — never from the previous frame —
/// so tweens don't compound (a 4-frame `scale: 2` ends at 2×, not 16×).
fn apply_cut_tweened(
    app: &mut App,
    before: &Snapshot,
    cut: &Cut,
    t: f32,
    state: &mut AnimState,
    first_frame: bool,
) -> Result<(), String> {
    if let Some(cam) = &cut.camera {
        app.camera.orientation = before.orientation;
        app.camera.distance = before.distance;
        app.camera.ortho_scale = before.ortho_scale;
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
            let up = glam::Vec3::from(up);
            app.camera.set_up(up);
            app.base_up = up.normalize_or(glam::Vec3::Y);
        }
        let world_up = cut_world_up(app, cam);
        if cam.azimuth_to.is_some() || cam.elevation_to.is_some() {
            // Absolute target pose, computed once from the base pose; the
            // tween slerps from the PRE-CUT orientation toward that target.
            let mut target = app.camera.clone();
            if let Some(view) = cam.view.as_deref().and_then(meshtui_core::ViewAxis::parse) {
                target.set_view_axis(view);
            } else {
                target.orientation = app.base_orientation;
            }
            target.orbit_around(
                world_up,
                cam.azimuth_to.unwrap_or(0.0).to_radians(),
                cam.elevation_to.unwrap_or(0.0).to_radians(),
            );
            app.camera.orientation = before.orientation.slerp(target.orientation, t);
        } else {
            if let Some(view) = cam.view.as_deref().and_then(meshtui_core::ViewAxis::parse) {
                app.camera.set_view_axis(view);
            }
            if cam.azimuth.is_some() || cam.elevation.is_some() {
                app.camera.orbit_around(
                    world_up,
                    cam.azimuth.unwrap_or(0.0).to_radians() * t,
                    cam.elevation.unwrap_or(0.0).to_radians() * t,
                );
            }
        }
        if let Some(zoom) = cam.zoom {
            app.camera.zoom(1.0 + (zoom - 1.0) * t);
        }
        if let Some(distance) = cam.distance {
            app.camera.distance = (before.distance + (distance - before.distance) * t).max(1e-3);
        }
    }
    for entry in &cut.meshes {
        // A `path` reload isn't interpolatable: swap the geometry once, on
        // the first frame of the cut.
        if let Some(path) = entry.path.as_deref() {
            if first_frame {
                reload_mesh(app, state, entry, path)?;
            }
            continue;
        }
        let idx = entry
            .name
            .as_deref()
            .and_then(|n| app.scene.meshes.iter().position(|m| m.name == n))
            .or_else(|| entry.name.as_deref()?.parse::<usize>().ok());
        let Some(i) = idx else { continue };

        // Transform: absolute against the base geometry, scaled by t.
        if entry.scale.is_some() || entry.translate.is_some() {
            let scale = 1.0 + (entry.scale.unwrap_or(1.0) - 1.0) * t;
            let tr = entry
                .translate
                .map(glam::Vec3::from)
                .unwrap_or(glam::Vec3::ZERO)
                * t;
            if let Some(base) = state.base_positions.get(i) {
                let mesh = &mut app.scene.meshes[i];
                if base.len() == mesh.positions.len() {
                    for (p, b) in mesh.positions.iter_mut().zip(base) {
                        *p = *b * scale + tr;
                    }
                    mesh.compute_normals();
                }
            }
        }
        // Color/alpha: lerp from the pre-cut snapshot to the cut's absolute
        // end value (computed from the entry, not the mutated mesh).
        if entry.color.is_some() || entry.alpha.is_some() {
            let Some(start) = before.mesh_colors.get(i).copied() else {
                continue;
            };
            let mut end = start;
            if let Some(color) = &entry.color {
                end = color.0;
            }
            if let Some(alpha) = entry.alpha {
                end[3] = alpha.clamp(0.0, 1.0);
            }
            let mesh = &mut app.scene.meshes[i];
            for ch in 0..4 {
                mesh.color[ch] = start[ch] + (end[ch] - start[ch]) * t;
            }
        }
        // Visibility can't lerp: it flips when the tween completes.
        if t >= 1.0 {
            if let Some(visible) = entry.visible {
                app.scene.meshes[i].visible = visible;
            }
        }
    }
    if let Some(light) = cut.light {
        app.set_light_scale(before.light + (light - before.light) * t);
    }
    if let Some(wireframe) = cut.wireframe {
        app.set_wireframe_thickness(before.wireframe + (wireframe - before.wireframe) * t);
    }
    Ok(())
}

/// Apply a mesh cut's fields (only the ones present) to a live mesh. `base`
/// is the mesh's cut-relative base geometry so scale/translate are ABSOLUTE
/// against the loaded mesh, never cumulative across cuts.
fn apply_mesh_cut(mesh: &mut meshtui_core::Mesh, entry: &MeshEntry, base: Option<&[glam::Vec3]>) {
    if let Some(color) = &entry.color {
        mesh.color = color.0;
    }
    if let Some(alpha) = entry.alpha {
        mesh.color[3] = alpha.clamp(0.0, 1.0);
    }
    if let Some(visible) = entry.visible {
        mesh.visible = visible;
    }
    apply_transform(mesh, entry, base);
}

/// Apply an absolute scale/translate against `base` geometry (the mesh's
/// current positions when no base is known).
fn apply_transform(mesh: &mut meshtui_core::Mesh, entry: &MeshEntry, base: Option<&[glam::Vec3]>) {
    if entry.scale.is_none() && entry.translate.is_none() {
        return;
    }
    let scale = entry.scale.unwrap_or(1.0);
    let t = entry
        .translate
        .map(glam::Vec3::from)
        .unwrap_or(glam::Vec3::ZERO);
    match base {
        Some(base) if base.len() == mesh.positions.len() => {
            for (p, b) in mesh.positions.iter_mut().zip(base) {
                *p = *b * scale + t;
            }
        }
        _ => {
            for p in &mut mesh.positions {
                *p = *p * scale + t;
            }
        }
    }
    mesh.compute_normals();
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

/// Convert straight alpha to GIF binary transparency with a stable 8x8 Bayer
/// pattern. Fully opaque pixels and their RGB values pass through unchanged.
fn dither_gif_alpha(pixels: &mut [u8], width: u32) {
    const BAYER_8X8: [[u8; 8]; 8] = [
        [0, 48, 12, 60, 3, 51, 15, 63],
        [32, 16, 44, 28, 35, 19, 47, 31],
        [8, 56, 4, 52, 11, 59, 7, 55],
        [40, 24, 36, 20, 43, 27, 39, 23],
        [2, 50, 14, 62, 1, 49, 13, 61],
        [34, 18, 46, 30, 33, 17, 45, 29],
        [10, 58, 6, 54, 9, 57, 5, 53],
        [42, 26, 38, 22, 41, 25, 37, 21],
    ];

    let width = width.max(1) as usize;
    for (i, px) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        if px[3] == 255 {
            continue;
        }
        let x = i % width;
        let y = i / width;
        let threshold = BAYER_8X8[y % 8][x % 8] * 4 + 2;
        if px[3] <= threshold {
            *px = [0, 0, 0, 0];
        } else {
            px[3] = 255;
        }
    }
}

/// Encode the rendered frames as an animated GIF.
///
/// GIF has 1-bit alpha. Fully transparent and fully opaque pixels are kept;
/// intermediate alpha is converted to binary coverage with deterministic
/// ordered Bayer dithering. `Frame::from_rgba_speed` supplies the matching
/// transparent palette index.
pub fn encode_gif(anim: &RenderedAnimation, out: &Path) -> Result<(), String> {
    use gif::{Encoder, Frame, Repeat};
    // Write to a temp file and rename on success so a failure never leaves a
    // truncated GIF at the output path.
    let tmp = out.with_extension("gif.tmp");
    let result = (|| -> Result<(), String> {
        let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut encoder = Encoder::new(&mut file, anim.width as u16, anim.height as u16, &[])
            .map_err(|e| e.to_string())?;
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|e| e.to_string())?;
        let delay_cs = (100 / anim.fps.max(1)) as u16; // hundredths of a second

        for raw in &anim.frames {
            let pixels = composite_frame(raw.clone(), anim.background);
            let mut frame = if anim.transparent {
                // Convert partial alpha to binary coverage before quantization.
                // from_rgba_speed creates and records the transparent index.
                let mut binned = pixels.clone();
                dither_gif_alpha(&mut binned, anim.width);
                Frame::from_rgba_speed(anim.width as u16, anim.height as u16, &mut binned, 10)
            } else {
                let mut rgb: Vec<u8> = Vec::with_capacity(pixels.len() / 4 * 3);
                for px in pixels.as_chunks::<4>().0 {
                    rgb.extend_from_slice(&px[..3]);
                }
                Frame::from_rgb_speed(anim.width as u16, anim.height as u16, &rgb, 10)
            };
            frame.delay = delay_cs;
            // With a transparent background, "keep" disposal would let earlier
            // frames show through this frame's transparent pixels (ghosting
            // trails). Restore-to-background clears them instead.
            frame.dispose = gif::DisposalMethod::Background;
            encoder.write_frame(&frame).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    // Move the temp file into place; on ANY failure (encode or rename) the
    // temp file is removed so no truncated artifact is left behind.
    let result = result.and_then(|()| std::fs::rename(&tmp, out).map_err(|e| e.to_string()));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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
    fn unknown_top_level_key_is_rejected() {
        let err = parse_str("meshes: [{path: a.ply}]\nfpss: 24\n").unwrap_err();
        assert!(err.contains("fpss"), "error names the typo: {err}");
        assert!(parse_str("meshes: [{path: a.ply}]\nfps: 24\n").is_ok());
    }

    #[test]
    fn unknown_ease_is_rejected() {
        let yaml =
            "meshes: [{path: a.ply}]\ncuts:\n  - frames: 4\n    tween: true\n    ease: inoout\n";
        let err = parse_str(yaml).unwrap_err();
        assert!(err.contains("inoout"), "error names the bad ease: {err}");
    }

    fn test_app() -> App {
        let mut mesh = meshtui_core::Mesh::new("m");
        mesh.positions = vec![
            glam::Vec3::X,
            glam::Vec3::new(0.0, 1.0, 0.0),
            glam::Vec3::ZERO,
        ];
        mesh.indices = vec![0, 1, 2];
        mesh.compute_normals();
        let mut scene = Scene::new();
        scene.meshes.push(mesh);
        App::new(scene, Config::default())
    }

    fn mesh_cut(yaml: &str) -> Cut {
        Cut {
            meshes: vec![serde_yml::from_str(yaml).unwrap()],
            ..Default::default()
        }
    }

    #[test]
    fn absolute_scale_does_not_compound_across_cuts() {
        let mut app = test_app();
        let mut state = AnimState::of(&app.scene);
        let cut = mesh_cut("name: m\nscale: 2.0");
        apply_cut(&mut app, &cut, &mut state).unwrap();
        apply_cut(&mut app, &cut, &mut state).unwrap();
        assert_eq!(
            app.scene.meshes[0].positions[0],
            glam::Vec3::new(2.0, 0.0, 0.0)
        );
    }

    #[test]
    fn tweened_scale_derives_from_base_not_previous_frame() {
        let mut app = test_app();
        let mut state = AnimState::of(&app.scene);
        let cut = mesh_cut("name: m\nscale: 2.0");
        let before = Snapshot::of(&app);
        for f in 0..4u32 {
            let t = (f + 1) as f32 / 4.0;
            apply_cut_tweened(&mut app, &before, &cut, t, &mut state, f == 0).unwrap();
            assert!((app.scene.meshes[0].positions[0].x - (1.0 + t)).abs() < 1e-6);
        }
    }

    #[test]
    fn tweened_color_lerps_from_precut_snapshot() {
        let mut app = test_app();
        app.scene.meshes[0].color = [0.0, 0.0, 0.0, 1.0];
        let mut state = AnimState::of(&app.scene);
        let cut = mesh_cut("name: m\ncolor: red");
        let before = Snapshot::of(&app);
        for f in 0..4u32 {
            let t = (f + 1) as f32 / 4.0;
            apply_cut_tweened(&mut app, &before, &cut, t, &mut state, f == 0).unwrap();
            assert!(
                (app.scene.meshes[0].color[0] - t).abs() < 1e-6,
                "frame {f}: red channel must be {t}, got {}",
                app.scene.meshes[0].color[0]
            );
        }
    }

    #[test]
    fn cut_reload_missing_path_is_an_error() {
        let mut app = test_app();
        let mut state = AnimState::of(&app.scene);
        let cut = mesh_cut("name: m\npath: /nonexistent/mesh.ply");
        let err = apply_cut(&mut app, &cut, &mut state).unwrap_err();
        assert!(err.contains("mesh.ply"), "error names the path: {err}");
    }

    #[test]
    fn cut_reload_preserves_omitted_color_and_applies_alpha() {
        let dir = std::env::temp_dir().join(format!("meshtui_reload_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stl = dir.join("swap.stl");
        std::fs::write(
            &stl,
            "solid s\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\n\
             vertex 0 1 0\nendloop\nendfacet\nendsolid s\n",
        )
        .unwrap();

        let mut app = test_app();
        app.scene.meshes[0].color = [0.2, 0.3, 0.4, 1.0];
        let mut state = AnimState::of(&app.scene);
        let cut = mesh_cut(&format!("name: m\npath: {}\nalpha: 0.2", stl.display()));
        apply_cut(&mut app, &cut, &mut state).unwrap();
        let c = app.scene.meshes[0].color;
        assert_eq!(&c[..3], &[0.2, 0.3, 0.4], "omitted color preserved");
        assert!((c[3] - 0.2).abs() < 1e-6, "explicit alpha applied");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn azimuth_to_resets_to_base_pose_without_named_view() {
        let mut app = test_app();
        let mut state = AnimState::of(&app.scene);
        let base = app.base_orientation;
        let relative = Cut {
            camera: Some(CameraCut {
                azimuth: Some(90.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        apply_cut(&mut app, &relative, &mut state).unwrap();
        assert!(app.camera.orientation.dot(base).abs() < 0.999);

        let absolute = Cut {
            camera: Some(CameraCut {
                azimuth_to: Some(0.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        apply_cut(&mut app, &absolute, &mut state).unwrap();
        assert!(
            app.camera.orientation.dot(base).abs() > 0.999_999,
            "azimuth_to: 0 returns to the base pose"
        );
    }

    #[test]
    fn relative_orbit_uses_world_up_not_tilted_camera_up() {
        let mut app = test_app();
        let mut state = AnimState::of(&app.scene);
        let tilt = Cut {
            camera: Some(CameraCut {
                elevation: Some(60.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        apply_cut(&mut app, &tilt, &mut state).unwrap();
        let before = app.camera.position();
        let turn = Cut {
            camera: Some(CameraCut {
                azimuth: Some(180.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        apply_cut(&mut app, &turn, &mut state).unwrap();
        let after = app.camera.position();
        // A pure azimuth turn keeps the camera at the same height (no tumble).
        assert!(
            (before.y - after.y).abs() < 1e-4,
            "azimuth around world-up keeps height: {before:?} -> {after:?}"
        );
    }

    #[test]
    fn ease_curves_endpoints_and_shape() {
        assert_eq!(ease(None, 0.0), 0.0);
        assert_eq!(ease(None, 1.0), 1.0);
        assert!((ease(Some("in"), 0.5) - 0.25).abs() < 1e-6);
        assert!((ease(Some("out"), 0.5) - 0.75).abs() < 1e-6);
        assert!((ease(Some("inout"), 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn gif_alpha_dither_preserves_binary_endpoints() {
        let mut pixels = vec![12, 34, 56, 0, 78, 90, 123, 255];
        dither_gif_alpha(&mut pixels, 2);
        assert_eq!(pixels, vec![0, 0, 0, 0, 78, 90, 123, 255]);

        let frame = gif::Frame::from_rgba_speed(2, 1, &mut pixels, 10);
        let transparent = frame.transparent.expect("transparent palette index");
        assert_eq!(frame.buffer[0], transparent);
        assert_ne!(frame.buffer[1], transparent);
    }

    #[test]
    fn gif_alpha_dither_gives_half_coverage_at_alpha_128() {
        let mut pixels = vec![0; 8 * 8 * 4];
        for px in pixels.as_chunks_mut::<4>().0.iter_mut() {
            *px = [64, 128, 192, 128];
        }
        dither_gif_alpha(&mut pixels, 8);

        let pixels = pixels.as_chunks::<4>().0;
        let transparent = pixels.iter().filter(|px| px[3] == 0).count();
        let opaque = pixels.iter().filter(|px| px[3] == 255).count();
        assert_eq!((transparent, opaque), (32, 32));
        assert!(pixels
            .iter()
            .filter(|px| px[3] == 0)
            .all(|px| px[..3] == [0, 0, 0]));
    }
}
