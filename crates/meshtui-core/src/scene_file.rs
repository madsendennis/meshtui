//! Scene-file format (YAML): declarative scene + camera + lighting + output,
//! loadable by the TUI (`meshtui scene.yaml`) and the headless renderers, and
//! the base state the animation cuts modify. This is the agent-facing scene
//! description.
//!
//! ```yaml
//! size: [1600, 1200]
//! background: "#1a1b26"        # or omit / `transparent: true`
//! transparent: false
//! camera:
//!   kind: orthographic          # orthographic | perspective
//!   view: "+z"                  # +x|-x|+y|-y|+z|-z  (or azimuth/elevation)
//!   azimuth: 30                 # degrees, alternative to view
//!   elevation: 20
//!   up: [0, 0, 1]               # Z-up parts
//!   fov: 60
//!   zoom: 0.9
//!   distance: 5.0
//! light: 1.0
//! wireframe: 0.0
//! meshes:
//!   - path: gear.ply            # or `source:`
//!     name: gear
//!     color: "#ff8000"          # name | #RRGGBB | #RRGGBBAA
//!     alpha: 1.0
//!     visible: true
//!     scale: 1.0
//!     translate: [0, 0, 0]
//! ```

use std::path::Path;

use serde::Deserialize;

use crate::{CameraKind, Mesh, Scene};

/// A parsed scene file. Output settings may be written flat (`size:`,
/// `background:`, `transparent:`) or nested under `output:`; both populate
/// `output`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct SceneFile {
    pub meshes: Vec<MeshEntry>,
    pub camera: CameraSpec,
    pub output: OutputSpec,
    /// Light intensity scale 0..=4 (like the TUI i/I).
    pub light: Option<f32>,
    /// Wireframe overlay thickness in pixels; 0 disables.
    pub wireframe: Option<f32>,

    // Flat top-level synonyms, folded into `output` after parsing.
    #[serde(rename = "size")]
    flat_size: Option<[u32; 2]>,
    #[serde(rename = "background")]
    flat_background: Option<ColorSpec>,
    #[serde(rename = "transparent")]
    flat_transparent: Option<bool>,
}

/// Output/output-composition settings, also settable top-level.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct OutputSpec {
    /// [width, height] in pixels.
    pub size: Option<[u32; 2]>,
    /// Background color; omit for transparent.
    pub background: Option<ColorSpec>,
    /// Force a transparent background (overrides `background` alpha).
    pub transparent: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CameraSpec {
    pub kind: Option<String>,
    pub view: Option<String>,
    pub azimuth: Option<f32>,
    pub elevation: Option<f32>,
    pub up: Option<[f32; 3]>,
    pub fov: Option<f32>,
    pub zoom: Option<f32>,
    pub distance: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MeshEntry {
    /// Mesh file or directory (`path` or `source` are synonyms).
    #[serde(alias = "source")]
    pub path: String,
    pub name: Option<String>,
    pub color: Option<ColorSpec>,
    pub alpha: Option<f32>,
    pub visible: Option<bool>,
    pub scale: Option<f32>,
    pub translate: Option<[f32; 3]>,
}

/// A color: palette name, `#RRGGBB`, or `#RRGGBBAA`.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorSpec(pub crate::Color);

impl<'de> Deserialize<'de> for ColorSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        crate::scene_file::parse_color(&s)
            .map(ColorSpec)
            .map_err(serde::de::Error::custom)
    }
}

/// Parse a color string into linear RGBA. Shared by the CLI flag parser and
/// the scene file so both accept the same names/hex.
pub fn parse_color(input: &str) -> Result<crate::Color, String> {
    let named = match input.to_ascii_lowercase().as_str() {
        "white" => Some([1.0, 1.0, 1.0, 1.0]),
        "red" => Some([1.0, 0.4, 0.4, 1.0]),
        "green" => Some([0.4, 1.0, 0.4, 1.0]),
        "blue" => Some([0.4, 0.6, 1.0, 1.0]),
        "yellow" => Some([1.0, 0.8, 0.2, 1.0]),
        "purple" => Some([0.8, 0.4, 1.0, 1.0]),
        "orange" => Some([1.0, 0.5, 0.0, 1.0]),
        "cyan" => Some([0.0, 0.8, 0.8, 1.0]),
        "gray" | "grey" => Some([0.5, 0.5, 0.5, 1.0]),
        "black" => Some([0.0, 0.0, 0.0, 1.0]),
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

/// Errors from parsing or applying a scene file.
#[derive(Debug, thiserror::Error)]
pub enum SceneError {
    #[error("failed to read {0}: {1}")]
    Io(std::path::PathBuf, std::io::Error),
    #[error("failed to parse scene file: {0}")]
    Parse(String),
    #[error("{0}")]
    Load(String),
}

/// Parse a scene file from YAML text.
pub fn parse_str(text: &str) -> Result<SceneFile, SceneError> {
    let mut file: SceneFile =
        serde_yml::from_str(text).map_err(|e| SceneError::Parse(e.to_string()))?;
    // Fold flat top-level output keys into `output` (nested wins).
    if file.output.size.is_none() {
        file.output.size = file.flat_size;
    }
    if file.output.background.is_none() {
        file.output.background = file.flat_background.clone();
    }
    if file.output.transparent.is_none() {
        file.output.transparent = file.flat_transparent;
    }
    Ok(file)
}

/// Load and parse a scene file from disk.
pub fn load(path: &Path) -> Result<SceneFile, SceneError> {
    let text = std::fs::read_to_string(path).map_err(|e| SceneError::Io(path.to_path_buf(), e))?;
    parse_str(&text)
}

impl SceneFile {
    /// Build the mesh scene: load each entry, apply overrides (name, color,
    /// alpha, visibility, uniform scale + translate).
    pub fn build_scene(&self) -> Result<Scene, SceneError> {
        let mut scene = Scene::new();
        for entry in &self.meshes {
            let mut meshes = crate::loaders::load_path(Path::new(&entry.path))
                .map_err(|e| SceneError::Load(e.to_string()))?;
            for mesh in &mut meshes {
                entry.apply(mesh);
            }
            scene.meshes.extend(meshes);
        }
        if scene.meshes.is_empty() {
            return Err(SceneError::Load("no geometry loaded".into()));
        }
        Ok(scene)
    }

    /// Camera kind override, if any.
    pub fn camera_kind(&self) -> Option<CameraKind> {
        self.camera.kind.as_deref().map(|k| match k {
            "perspective" | "persp" => CameraKind::Perspective,
            _ => CameraKind::Orthographic,
        })
    }
}

impl MeshEntry {
    /// Apply this entry's overrides to a loaded mesh.
    fn apply(&self, mesh: &mut Mesh) {
        if let Some(name) = &self.name {
            mesh.name = name.clone();
        }
        if let Some(color) = &self.color {
            mesh.color = color.0;
            mesh.original_color = Some(color.0);
        }
        if let Some(alpha) = self.alpha {
            mesh.color[3] *= alpha.clamp(0.0, 1.0);
        }
        if let Some(visible) = self.visible {
            mesh.visible = visible;
        }
        if self.scale.is_some() || self.translate.is_some() {
            let scale = self.scale.unwrap_or(1.0);
            let t = self
                .translate
                .map(glam::Vec3::from)
                .unwrap_or(glam::Vec3::ZERO);
            for p in &mut mesh.positions {
                *p = *p * scale + t;
            }
            mesh.compute_normals();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_scene() {
        let file = parse_str("meshes:\n  - path: a.ply\n").unwrap();
        assert_eq!(file.meshes.len(), 1);
        assert_eq!(file.meshes[0].path, "a.ply");
    }

    #[test]
    fn parses_full_scene_with_camera_and_colors() {
        let yaml = r##"
size: [800, 600]
transparent: true
camera:
  kind: perspective
  view: "+z"
  up: [0, 0, 1]
  zoom: 0.8
light: 2.0
wireframe: 1.5
meshes:
  - path: gear.ply
    name: gear
    color: "#ff8000"
    alpha: 0.5
    visible: false
    scale: 2.0
    translate: [1, 0, 0]
"##;
        let f = parse_str(yaml).unwrap();
        assert_eq!(f.output.size, Some([800, 600]));
        assert_eq!(f.output.transparent, Some(true));
        assert_eq!(f.camera.kind.as_deref(), Some("perspective"));
        assert_eq!(f.camera.up, Some([0.0, 0.0, 1.0]));
        assert_eq!(f.light, Some(2.0));
        assert_eq!(f.wireframe, Some(1.5));
        let m = &f.meshes[0];
        assert_eq!(m.name.as_deref(), Some("gear"));
        assert_eq!(
            m.color.as_ref().map(|c| c.0),
            Some([1.0, 128.0 / 255.0, 0.0, 1.0])
        );
        assert_eq!(m.alpha, Some(0.5));
        assert_eq!(m.visible, Some(false));
        assert_eq!(m.scale, Some(2.0));
    }

    #[test]
    fn color_spec_parses_names_and_hex() {
        assert_eq!(parse_color("orange").unwrap(), [1.0, 0.5, 0.0, 1.0]);
        assert_eq!(parse_color("#00ff0080").unwrap()[3], 128.0 / 255.0);
        assert!(parse_color("nope").is_err());
    }

    #[test]
    fn mesh_entry_applies_transform_and_color() {
        let entry: MeshEntry =
            serde_yml::from_str("path: x.ply\nname: m\ncolor: red\nscale: 2\ntranslate: [1,0,0]")
                .unwrap();
        let mut mesh = Mesh::new("orig");
        mesh.positions = vec![glam::Vec3::X];
        mesh.indices = vec![0, 0, 0];
        entry.apply(&mut mesh);
        assert_eq!(mesh.name, "m");
        assert_eq!(mesh.positions[0], glam::Vec3::new(3.0, 0.0, 0.0));
        assert_eq!(mesh.color, [1.0, 0.4, 0.4, 1.0]);
    }
}
