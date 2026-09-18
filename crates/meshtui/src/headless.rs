//! Headless scene setup: apply CLI flags and per-mesh overrides so every
//! TUI setting is reachable from the terminal. Shared by the screenshot and
//! animate paths.

use std::path::Path;

use glam::Vec3;
use meshtui_core::camera::ViewAxis;
use meshtui_core::{Color, Scene};

use crate::app::App;

/// A per-mesh override parsed from `path:key=value,...` or
/// `name=path:key=value,...`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshSpec {
    /// Optional explicit mesh name (defaults to the file stem).
    pub name: Option<String>,
    pub path: String,
    pub color: Option<Color>,
    /// Per-mesh opacity 0..=1; multiplies the color alpha.
    pub alpha: Option<f32>,
    pub visible: Option<bool>,
}

/// Camera/scene overrides from CLI flags. `None` means "keep config default".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HeadlessOpts {
    pub camera_kind: Option<&'static str>,
    pub view: Option<ViewAxis>,
    /// Camera direction as azimuth/elevation in degrees around the up axis.
    pub azimuth: Option<f32>,
    pub elevation: Option<f32>,
    pub up: Option<Vec3>,
    pub fov: Option<f32>,
    /// Zoom factor (< 1 zooms in, > 1 zooms out), like the TUI `z`/`Z`.
    pub zoom: Option<f32>,
    /// Explicit target→camera distance (overrides the auto fit).
    pub distance: Option<f32>,
    /// Light intensity scale, like the TUI `i`/`I`.
    pub light: Option<f32>,
    /// Wireframe thickness in pixels; 0 disables.
    pub wireframe: Option<f32>,
    /// Background RGBA 0-255; `--transparent` maps to alpha 0.
    pub background: Option<[u8; 4]>,
    pub transparent: bool,
}

/// Parse a mesh spec: `[name=]path[:key=value,...]`.
/// Windows drive letters are not a concern (paths come from the CLI).
pub fn parse_mesh_spec(input: &str) -> Result<MeshSpec, String> {
    let (name, rest) = match input.split_once('=') {
        // `name=path` only when the left side has no path separator/colon.
        Some((left, right)) if !left.contains(['/', ':', '.']) => (Some(left.to_string()), right),
        _ => (None, input),
    };
    let mut parts = rest.splitn(2, ':');
    let path = parts.next().unwrap_or_default().to_string();
    if path.is_empty() {
        return Err(format!("mesh spec {input:?} has no path"));
    }
    let mut spec = MeshSpec {
        name,
        path,
        ..Default::default()
    };
    if let Some(opts) = parts.next() {
        for kv in opts.split(':') {
            let (key, value) = kv
                .split_once('=')
                .ok_or_else(|| format!("mesh spec option {kv:?} must be key=value"))?;
            match key {
                "color" => spec.color = Some(parse_color(value)?),
                "alpha" => {
                    spec.alpha = Some(
                        value
                            .parse::<f32>()
                            .ok()
                            .filter(|a| (0.0..=1.0).contains(a))
                            .ok_or("alpha must be 0..=1")?,
                    )
                }
                "visible" => {
                    spec.visible = Some(match value {
                        "true" | "1" | "yes" => true,
                        "false" | "0" | "no" => false,
                        _ => return Err("visible must be true/false".into()),
                    })
                }
                other => return Err(format!("unknown mesh option {other:?}")),
            }
        }
    }
    Ok(spec)
}

/// Parse a color: palette name, `#RRGGBB`, or `#RRGGBBAA`.
pub fn parse_color(input: &str) -> Result<Color, String> {
    let named = match input.to_ascii_lowercase().as_str() {
        "white" => Some([1.0, 1.0, 1.0, 1.0]),
        "red" => Some([1.0, 0.4, 0.4, 1.0]),
        "green" => Some([0.4, 1.0, 0.4, 1.0]),
        "blue" => Some([0.4, 0.6, 1.0, 1.0]),
        "yellow" => Some([1.0, 0.8, 0.2, 1.0]),
        "purple" => Some([0.8, 0.4, 1.0, 1.0]),
        "orange" => Some([1.0, 0.5, 0.0, 1.0]),
        "cyan" => Some([0.0, 0.8, 0.8, 1.0]),
        _ => None,
    };
    if let Some(c) = named {
        return Ok(c);
    }
    let hex = input.strip_prefix('#').unwrap_or(input);
    let value = u32::from_str_radix(hex, 16).map_err(|_| format!("invalid color {input:?}"))?;
    match hex.len() {
        6 => Ok([
            ((value >> 16) & 0xff) as f32 / 255.0,
            ((value >> 8) & 0xff) as f32 / 255.0,
            (value & 0xff) as f32 / 255.0,
            1.0,
        ]),
        8 => Ok([
            ((value >> 24) & 0xff) as f32 / 255.0,
            ((value >> 16) & 0xff) as f32 / 255.0,
            ((value >> 8) & 0xff) as f32 / 255.0,
            (value & 0xff) as f32 / 255.0,
        ]),
        _ => Err(format!(
            "color {input:?} must be a name, #RRGGBB, or #RRGGBBAA"
        )),
    }
}

/// Load meshes for the given specs (or bare paths), applying overrides.
pub fn load_scene(specs: &[MeshSpec]) -> Result<Scene, String> {
    let mut scene = Scene::new();
    for spec in specs {
        let meshes =
            meshtui_core::loaders::load_path(Path::new(&spec.path)).map_err(|e| e.to_string())?;
        for mut mesh in meshes {
            if let Some(name) = &spec.name {
                mesh.name = name.clone();
            }
            if let Some(color) = spec.color {
                mesh.color = color;
                mesh.original_color = Some(color);
            }
            if let Some(alpha) = spec.alpha {
                mesh.color[3] *= alpha;
            }
            if let Some(visible) = spec.visible {
                mesh.visible = visible;
            }
            scene.meshes.push(mesh);
        }
    }
    if scene.meshes.is_empty() {
        return Err("no geometry loaded".into());
    }
    Ok(scene)
}

/// Apply camera/scene overrides to a freshly built App.
pub fn apply_headless(app: &mut App, opts: &HeadlessOpts) {
    if let Some(kind) = opts.camera_kind {
        let kind = match kind {
            "ortho" | "orthographic" => meshtui_core::CameraKind::Orthographic,
            _ => meshtui_core::CameraKind::Perspective,
        };
        if app.camera.kind != kind {
            app.camera.kind = kind;
        }
    }
    if let Some(fov) = opts.fov {
        app.camera.fov_degrees = fov;
    }
    // The world up vector is the orbit reference. `set_up` re-orients the
    // whole camera to it (tumbles the world), establishing the frame the
    // view/azimuth/elevation below build on.
    if let Some(up) = opts.up {
        app.camera.set_up(up);
    }
    if let Some(axis) = opts.view {
        app.camera.set_view_axis(axis);
    }
    if opts.azimuth.is_some() || opts.elevation.is_some() {
        // Orbit RELATIVE to the current (view) pose: azimuth turns around the
        // world up axis, elevation tilts around the camera's right axis. The
        // base view (or default) is the az=0/el=0 reference, so
        // `--view -x --azimuth 180` lands exactly opposite that view.
        let az = opts.azimuth.unwrap_or(0.0).to_radians();
        let el = opts.elevation.unwrap_or(0.0).to_radians();
        let up = opts.up.unwrap_or_else(|| app.camera.up());
        app.camera.orbit_around(up, az, el);
    }
    if let Some(zoom) = opts.zoom {
        app.camera.zoom(zoom);
    }
    if let Some(distance) = opts.distance {
        app.camera.distance = distance.max(1e-3);
    }
    if let Some(light) = opts.light {
        app.set_light_scale(light);
    }
    if let Some(wireframe) = opts.wireframe {
        app.set_wireframe_thickness(wireframe);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_spec_parses_path_only() {
        let spec = parse_mesh_spec("model.ply").unwrap();
        assert_eq!(spec.path, "model.ply");
        assert_eq!(spec.name, None);
    }

    #[test]
    fn mesh_spec_parses_name_and_options() {
        let spec =
            parse_mesh_spec("gear=parts/gear.ply:color=#ff0000:alpha=0.5:visible=true").unwrap();
        assert_eq!(spec.name.as_deref(), Some("gear"));
        assert_eq!(spec.path, "parts/gear.ply");
        assert_eq!(spec.color, Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(spec.alpha, Some(0.5));
        assert_eq!(spec.visible, Some(true));
    }

    #[test]
    fn mesh_spec_parses_named_color() {
        let spec = parse_mesh_spec("m.ply:color=blue").unwrap();
        assert_eq!(spec.color, Some([0.4, 0.6, 1.0, 1.0]));
    }

    #[test]
    fn mesh_spec_rejects_bad_option() {
        assert!(parse_mesh_spec("m.ply:bogus=1").is_err());
        assert!(parse_mesh_spec("m.ply:alpha=2").is_err());
        assert!(parse_mesh_spec("m.ply:visible=maybe").is_err());
    }

    #[test]
    fn color_parses_hex_with_alpha() {
        assert_eq!(parse_color("#ff000080").unwrap()[3], 128.0 / 255.0);
        assert_eq!(parse_color("#00ff00").unwrap()[1], 1.0);
    }
}
