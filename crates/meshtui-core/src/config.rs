//! TOML configuration, schema-compatible with the Python version's
//! `default_config.toml`. Differences (deliberate fixes):
//! - the duplicate `mesh_reset = "r"` binding is gone (it shadowed
//!   `reset_single_mesh`);
//! - `theme` is read from `[ui.sidepanel].theme` where the file actually
//!   defines it (the Python code read the wrong section and ignored it).

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::Color;

pub const DEFAULT_CONFIG: &str = include_str!("../default_config.toml");

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub camera: CameraConfig,
    pub orbital_camera: OrbitalConfig,
    pub view: ViewConfig,
    pub material: MaterialConfig,
    pub scene: SceneConfig,
    pub lighting: LightingConfig,
    pub wireframe: WireframeConfig,
    pub ui: UiConfig,
    pub terminal: TerminalConfig,
    pub keybindings: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CameraConfig {
    #[serde(rename = "type")]
    pub kind: CameraKindConfig,
    pub fov_degrees: f32,
    pub distance_padding: f32,
    pub initial_theta: f32,
    pub initial_phi: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraKindConfig {
    Perspective,
    Orthographic,
}

impl<'de> Deserialize<'de> for CameraKindConfig {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "perspective" => Ok(Self::Perspective),
            "orthographic" => Ok(Self::Orthographic),
            other => Err(serde::de::Error::custom(format!(
                "unknown camera type {other}"
            ))),
        }
    }
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            kind: CameraKindConfig::Orthographic,
            fov_degrees: 60.0,
            distance_padding: 1.0,
            initial_theta: 0.0,
            initial_phi: std::f32::consts::FRAC_PI_2,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OrbitalConfig {
    pub movement_speed: f32,
    pub movement_speed_fast_multiplier: f32,
    pub zoom_in_factor: f32,
    pub zoom_out_factor: f32,
}

impl Default for OrbitalConfig {
    fn default() -> Self {
        Self {
            movement_speed: 0.1,
            movement_speed_fast_multiplier: 5.0,
            zoom_in_factor: 0.9,
            zoom_out_factor: 1.1,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ViewConfig {
    pub default_axis: String,
    pub up_vectors: Vec<[f32; 3]>,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            default_axis: "+y".into(),
            up_vectors: vec![[0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MaterialConfig {
    pub base_color: Color,
    pub roughness_factor: f32,
}

impl Default for MaterialConfig {
    fn default() -> Self {
        Self {
            base_color: [0.85, 0.85, 0.87, 1.0],
            roughness_factor: 0.7,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SceneConfig {
    pub ambient_light: [f32; 3],
    pub bg_color_dark: [u8; 4],
    pub bg_color_light: [u8; 4],
}

impl Default for SceneConfig {
    fn default() -> Self {
        Self {
            ambient_light: [0.15, 0.15, 0.15],
            bg_color_dark: [26, 27, 38, 0],
            bg_color_light: [255, 255, 255, 0],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LightingConfig {
    pub key_light_intensity: f32,
    pub fill_light_intensity: f32,
    pub fill_light_azimuth: f32,
    pub fill_light_elevation: f32,
    pub rim_light_intensity: f32,
    pub rim_light_azimuth: f32,
    pub rim_light_elevation: f32,
}

impl Default for LightingConfig {
    fn default() -> Self {
        Self {
            key_light_intensity: 1.5,
            fill_light_intensity: 1.5,
            fill_light_azimuth: -45.0,
            fill_light_elevation: 15.0,
            rim_light_intensity: 0.6,
            rim_light_azimuth: 135.0,
            rim_light_elevation: 10.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WireframeConfig {
    pub default_thickness: f32,
    pub color_dark_bg: [u8; 4],
    pub color_light_bg: [u8; 4],
}

impl Default for WireframeConfig {
    fn default() -> Self {
        Self {
            default_thickness: 0.0,
            color_dark_bg: [60, 60, 60, 255],
            color_light_bg: [20, 20, 20, 255],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct UiConfig {
    pub sidepanel: SidepanelConfig,
    pub animation: AnimationConfig,
    pub command_palette: CommandPaletteConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SidepanelConfig {
    pub theme: String,
    pub show_scene_info: bool,
    pub scene_info_toggle_key: String,
    pub color_palette: Vec<Color>,
}

impl Default for SidepanelConfig {
    fn default() -> Self {
        Self {
            theme: "omarchy".into(),
            show_scene_info: true,
            scene_info_toggle_key: ".".into(),
            color_palette: vec![
                [1.0, 1.0, 1.0, 1.0],
                [1.0, 0.4, 0.4, 1.0],
                [0.4, 1.0, 0.4, 1.0],
                [0.4, 0.6, 1.0, 1.0],
                [1.0, 0.8, 0.2, 1.0],
                [0.8, 0.4, 1.0, 1.0],
                [1.0, 0.5, 0.0, 1.0],
                [0.0, 0.8, 0.8, 1.0],
            ],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AnimationConfig {
    pub transition_duration_ms: u64,
    pub animation_fps: u32,
    pub render_on_animate: bool,
    pub default_interval_ms: u64,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            transition_duration_ms: 300,
            animation_fps: 60,
            render_on_animate: true,
            default_interval_ms: 100,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CommandPaletteConfig {
    pub fuzzy_match_threshold: f32,
    pub max_visible_items: usize,
}

impl Default for CommandPaletteConfig {
    fn default() -> Self {
        Self {
            fuzzy_match_threshold: 0.2,
            max_visible_items: 20,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub estimated_cell_width_px: u32,
    pub estimated_cell_height_px: u32,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            estimated_cell_width_px: 10,
            estimated_cell_height_px: 20,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            camera: CameraConfig::default(),
            orbital_camera: OrbitalConfig::default(),
            view: ViewConfig::default(),
            material: MaterialConfig::default(),
            scene: SceneConfig::default(),
            lighting: LightingConfig::default(),
            wireframe: WireframeConfig::default(),
            ui: UiConfig::default(),
            terminal: TerminalConfig::default(),
            keybindings: default_keybindings(),
        }
    }
}

fn default_keybindings() -> HashMap<String, toml::Value> {
    let pairs: &[(&str, &str)] = &[
        ("quit", "q"),
        ("toggle_sidepanel", "tab"),
        ("view_minus_x", "1"),
        ("view_plus_x", "2"),
        ("view_minus_y", "3"),
        ("view_plus_y", "4"),
        ("view_minus_z", "5"),
        ("view_plus_z", "6"),
        ("wireframe_off", "g"),
        ("wireframe_increase", "G"),
        ("camera_orthographic", "w"),
        ("camera_perspective", "W"),
        ("light_decrease", "i"),
        ("light_increase", "I"),
        ("up_vector_next", "u"),
        ("up_vector_prev", "U"),
        ("orbit_left", "h"),
        ("orbit_right", "l"),
        ("orbit_down", "j"),
        ("orbit_up", "k"),
        ("orbit_left_fast", "H"),
        ("orbit_right_fast", "L"),
        ("orbit_down_fast", "J"),
        ("orbit_up_fast", "K"),
        ("zoom_in", "z"),
        ("zoom_out", "Z"),
        ("reset_single_mesh", "r"),
        ("reset_all_meshes", "R"),
        ("reset_orbital", "0"),
        ("screenshot", "p"),
        ("toggle_scene_info", "."),
        ("mesh_hide", "s"),
        ("mesh_show", "S"),
        ("mesh_color_next", "c"),
        ("mesh_color_prev", "C"),
        ("mesh_custom_color", "x"),
        ("mesh_toggle_mark", "space"),
        ("mesh_select_all", "ctrl+a"),
        ("mesh_select_none", "ctrl+n"),
        ("mesh_filter", "/"),
        ("animation_start", "a"),
        ("animation_stop", "A"),
        ("sidepanel_move_up", "up"),
        ("sidepanel_move_down", "down"),
    ];
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), toml::Value::String(v.to_string())))
        .collect()
}

impl Config {
    /// Load the embedded defaults, optionally merged with a user TOML file
    /// (user values override defaults, section-wise via deep merge).
    pub fn load(user_path: Option<&Path>) -> Result<Self, ConfigError> {
        let mut base: toml::Value = toml::from_str(DEFAULT_CONFIG)
            .map_err(|e| ConfigError::Parse("<default>".into(), e))?;
        if let Some(path) = user_path {
            let text = std::fs::read_to_string(path)
                .map_err(|e| ConfigError::Io(path.to_path_buf(), e))?;
            let user: toml::Value = toml::from_str(&text)
                .map_err(|e| ConfigError::Parse(path.display().to_string(), e))?;
            merge_toml(&mut base, user);
        }
        let cfg: Config = base
            .try_into()
            .map_err(|e| ConfigError::Parse("<merged>".into(), e))?;
        cfg.validate_values()?;
        cfg.validate_keybindings()?;
        Ok(cfg)
    }

    /// Resolve an action name to its key string (from `[keybindings]`).
    pub fn key(&self, action: &str) -> Option<String> {
        match self.keybindings.get(action)? {
            toml::Value::String(s) => Some(s.clone()),
            toml::Value::Array(a) => a.first().and_then(|v| v.as_str()).map(str::to_string),
            _ => None,
        }
    }

    /// Check for duplicate keys across actions — the Python version had
    /// `reset_single_mesh` and `mesh_reset` both on "r".
    pub fn validate_keybindings(&self) -> Result<(), ConfigError> {
        let mut seen: HashMap<String, String> = HashMap::new();
        for (action, value) in &self.keybindings {
            let keys: Vec<String> = match value {
                toml::Value::String(s) => vec![s.clone()],
                toml::Value::Array(a) => a
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
                _ => {
                    return Err(ConfigError::Invalid(format!(
                        "keybinding {action:?} must be a string or array of strings"
                    )))
                }
            };
            if keys.is_empty()
                || keys.iter().any(String::is_empty)
                || value
                    .as_array()
                    .is_some_and(|items| items.len() != keys.len())
            {
                return Err(ConfigError::Invalid(format!(
                    "keybinding {action:?} must contain non-empty strings"
                )));
            }
            for key in keys {
                if let Some(other) = seen.get(&key) {
                    return Err(ConfigError::DuplicateKey {
                        key,
                        first: other.clone(),
                        second: action.clone(),
                    });
                }
                seen.insert(key, action.clone());
            }
        }
        Ok(())
    }

    fn validate_values(&self) -> Result<(), ConfigError> {
        let finite_positive = |name: &str, value: f32| {
            if value.is_finite() && value > 0.0 {
                Ok(())
            } else {
                Err(ConfigError::Invalid(format!(
                    "{name} must be finite and greater than zero"
                )))
            }
        };
        if !(self.camera.fov_degrees.is_finite()
            && self.camera.fov_degrees > 0.0
            && self.camera.fov_degrees < 179.0)
        {
            return Err(ConfigError::Invalid(
                "camera.fov_degrees must be between 0 and 179".into(),
            ));
        }
        finite_positive("camera.distance_padding", self.camera.distance_padding)?;
        if !self.camera.initial_theta.is_finite()
            || !(0.0..std::f32::consts::PI).contains(&self.camera.initial_phi)
        {
            return Err(ConfigError::Invalid(
                "camera angles must be finite and initial_phi must be between 0 and pi".into(),
            ));
        }
        finite_positive(
            "orbital_camera.zoom_in_factor",
            self.orbital_camera.zoom_in_factor,
        )?;
        finite_positive(
            "orbital_camera.zoom_out_factor",
            self.orbital_camera.zoom_out_factor,
        )?;
        finite_positive(
            "orbital_camera.movement_speed_fast_multiplier",
            self.orbital_camera.movement_speed_fast_multiplier,
        )?;
        if !(self.orbital_camera.movement_speed.is_finite()
            && self.orbital_camera.movement_speed >= 0.0)
        {
            return Err(ConfigError::Invalid(
                "orbital_camera.movement_speed must be finite and non-negative".into(),
            ));
        }
        if !(self.material.roughness_factor.is_finite()
            && (0.0..=1.0).contains(&self.material.roughness_factor))
        {
            return Err(ConfigError::Invalid(
                "material.roughness_factor must be between 0 and 1".into(),
            ));
        }
        if self
            .material
            .base_color
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(ConfigError::Invalid(
                "material.base_color channels must be between 0 and 1".into(),
            ));
        }
        if self.view.up_vectors.is_empty()
            || self.view.up_vectors.iter().any(|v| {
                let v = glam::Vec3::from(*v);
                !v.is_finite() || v.length_squared() <= f32::EPSILON
            })
        {
            return Err(ConfigError::Invalid(
                "view.up_vectors must contain finite, non-zero vectors".into(),
            ));
        }
        if crate::camera::ViewAxis::parse(&self.view.default_axis).is_none() {
            return Err(ConfigError::Invalid(
                "view.default_axis must be one of +x, -x, +y, -y, +z, or -z".into(),
            ));
        }
        if self
            .scene
            .ambient_light
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(ConfigError::Invalid(
                "scene.ambient_light channels must be between 0 and 1".into(),
            ));
        }
        if self
            .ui
            .sidepanel
            .color_palette
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(ConfigError::Invalid(
                "ui.sidepanel.color_palette channels must be between 0 and 1".into(),
            ));
        }
        if !(1..=240).contains(&self.ui.animation.animation_fps)
            || !(1..=10_000).contains(&self.ui.animation.default_interval_ms)
        {
            return Err(ConfigError::Invalid(
                "ui.animation.animation_fps must be 1..240 and default_interval_ms must be 1..10000"
                    .into(),
            ));
        }
        if !(self.ui.command_palette.fuzzy_match_threshold.is_finite()
            && (0.0..=1.0).contains(&self.ui.command_palette.fuzzy_match_threshold))
            || self.ui.command_palette.max_visible_items == 0
        {
            return Err(ConfigError::Invalid(
                "ui.command_palette threshold must be 0..1 and max_visible_items must be positive"
                    .into(),
            ));
        }
        for (name, value) in [
            (
                "lighting.fill_light_azimuth",
                self.lighting.fill_light_azimuth,
            ),
            (
                "lighting.fill_light_elevation",
                self.lighting.fill_light_elevation,
            ),
            (
                "lighting.rim_light_azimuth",
                self.lighting.rim_light_azimuth,
            ),
            (
                "lighting.rim_light_elevation",
                self.lighting.rim_light_elevation,
            ),
        ] {
            if !value.is_finite() {
                return Err(ConfigError::Invalid(format!("{name} must be finite")));
            }
        }
        for (name, value) in [
            (
                "lighting.key_light_intensity",
                self.lighting.key_light_intensity,
            ),
            (
                "lighting.fill_light_intensity",
                self.lighting.fill_light_intensity,
            ),
            (
                "lighting.rim_light_intensity",
                self.lighting.rim_light_intensity,
            ),
            (
                "wireframe.default_thickness",
                self.wireframe.default_thickness,
            ),
        ] {
            if !(value.is_finite() && value >= 0.0) {
                return Err(ConfigError::Invalid(format!(
                    "{name} must be finite and non-negative"
                )));
            }
        }
        if self.terminal.estimated_cell_width_px == 0 || self.terminal.estimated_cell_height_px == 0
        {
            return Err(ConfigError::Invalid(
                "terminal estimated cell dimensions must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read {0}: {1}")]
    Io(std::path::PathBuf, std::io::Error),
    #[error("failed to parse {0}: {1}")]
    Parse(String, toml::de::Error),
    #[error("duplicate keybinding: key {key:?} maps to both {first:?} and {second:?}")]
    DuplicateKey {
        key: String,
        first: String,
        second: String,
    },
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

fn merge_toml(base: &mut toml::Value, over: toml::Value) {
    match (base, over) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(bv) => merge_toml(bv, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, o) => *b = o,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse() {
        let cfg = Config::load(None).unwrap();
        assert_eq!(cfg.key("orbit_left").as_deref(), Some("h"));
        // theme is read from [ui.sidepanel] where the file defines it
        assert_eq!(cfg.ui.sidepanel.theme, "omarchy");
        assert_eq!(cfg.ui.animation.animation_fps, 60);
        assert_eq!(cfg.ui.command_palette.max_visible_items, 20);
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        let mut cfg = Config::default();
        cfg.keybindings
            .insert("mesh_reset".into(), toml::Value::String("r".into()));
        assert!(matches!(
            cfg.validate_keybindings(),
            Err(ConfigError::DuplicateKey { .. })
        ));
    }

    #[test]
    fn user_config_overrides() {
        let dir = std::env::temp_dir();
        let path = dir.join("meshtui_test_user.toml");
        std::fs::write(&path, "[keybindings]\norbit_left = \"b\"\n").unwrap();
        let cfg = Config::load(Some(&path)).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(cfg.key("orbit_left").as_deref(), Some("b"));
        // untouched defaults remain
        assert_eq!(cfg.key("orbit_right").as_deref(), Some("l"));
    }

    #[test]
    fn invalid_fov_is_rejected() {
        let mut cfg = Config::default();
        cfg.camera.fov_degrees = 180.0;
        assert!(matches!(
            cfg.validate_values(),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn excessive_animation_fps_is_rejected() {
        let mut cfg = Config::default();
        cfg.ui.animation.animation_fps = 1000;
        assert!(matches!(
            cfg.validate_values(),
            Err(ConfigError::Invalid(_))
        ));
    }
}
