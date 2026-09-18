//! Application state and the dirty-flag event loop.

use std::collections::{BTreeSet, VecDeque};
use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::{cursor, execute};
use glam::Vec3;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color as TColor, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame as TuiFrame};
use regex::RegexBuilder;

use meshtui_core::camera::ViewAxis;
use meshtui_core::config::Config;
use meshtui_core::loaders::load_path;
use meshtui_core::{Camera, CameraKind, Mesh, Scene};
use meshtui_render::software::{camera_light_offset, Lighting, Options, SoftwareRasterizer};
use meshtui_render::{Frame, RenderBackend};
use meshtui_term::{
    delete_image, encode_png_fallback, image_channel, resolve_theme, FileImage, ImageChannel,
    Theme, ThemeWatcher,
};

use crate::commands::{fuzzy_score, parse_hex_color, CommandSpec, COMMANDS};

const IMAGE_ID: u32 = 1;
/// Max pixels on the longest edge of a render.
const MAX_RENDER_DIM: u32 = 2048;

enum Modal {
    CommandPalette {
        query: String,
        selected: usize,
    },
    HexColor {
        input: String,
        error: Option<String>,
    },
    AnimationDelay {
        input: String,
        error: Option<String>,
    },
    AnimationDirection {
        interval_ms: u64,
    },
    MeshFilter {
        input: String,
        error: Option<String>,
    },
    OpenPath {
        input: String,
        error: Option<String>,
    },
}

/// Meshes removed by `mesh_delete`, restorable LIFO via `mesh_undo_delete`.
struct DeletedMeshes {
    /// (original scene index, mesh), ascending by index.
    entries: Vec<(usize, Mesh)>,
    /// Marks (original indices) that sat on the removed meshes.
    marked: BTreeSet<usize>,
}

struct AnimationState {
    action: &'static str,
    interval: Duration,
    next_step: Instant,
    render_interval: Duration,
    next_render: Instant,
    pending_render: bool,
}

pub struct App {
    pub scene: Scene,
    pub camera: Camera,
    pub config: Config,
    rasterizer: SoftwareRasterizer,
    selected: usize,
    marked: BTreeSet<usize>,
    mesh_filter: String,
    filtered_indices: Vec<usize>,
    deleted: Vec<DeletedMeshes>,
    show_sidepanel: bool,
    show_scene_info: bool,
    modal: Option<Modal>,
    animation: Option<AnimationState>,
    wireframe_thickness: f32,
    light_scale: f32,
    theme: Theme,
    status_message: Option<String>,
    /// Viewport pixel aspect (width/height) used to fit the camera so meshes
    /// fill wide viewports instead of swimming. 1.0 until the first render.
    aspect: f32,
    /// The base camera pose, captured after scene-camera setup. Absolute
    /// `azimuth_to`/`elevation_to` animation cuts reset to this orientation,
    /// so they work even when the scene never named a view axis.
    pub base_orientation: glam::Quat,
    /// The persistent world up axis orbits turn around (from config or the
    /// scene's camera.up). Not the camera's mutable, possibly tilted up.
    pub base_up: Vec3,
    /// Scene zoom/distance waiting for the one-time auto-fit (TUI scene
    /// open applies the camera before the terminal aspect is known).
    pending_zoom: Option<f32>,
    pending_distance: Option<f32>,
    /// When on (default), hiding/showing/deleting/adding meshes reframes the
    /// camera to the visible bounds. Toggle off (`,`) to keep the camera
    /// distance constant — e.g. while stepping meshes for an animation.
    auto_zoom: bool,
    dirty: bool,
    render_dirty: bool,
}

impl App {
    pub fn new(scene: Scene, config: Config) -> Self {
        let kind = match config.camera.kind {
            meshtui_core::config::CameraKindConfig::Perspective => CameraKind::Perspective,
            meshtui_core::config::CameraKindConfig::Orthographic => CameraKind::Orthographic,
        };
        let (min, max) = scene
            .visible_bounds()
            .unwrap_or((Vec3::splat(-1.0), Vec3::splat(1.0)));
        let mut camera = Camera::frame_bounds(
            min,
            max,
            kind,
            config.camera.fov_degrees,
            config.camera.distance_padding,
        );
        camera.set_spherical(
            config.camera.initial_theta,
            config.camera.initial_phi,
            Vec3::from(
                config
                    .view
                    .up_vectors
                    .first()
                    .copied()
                    .unwrap_or([0.0, 1.0, 0.0]),
            ),
        );
        if let Some(axis) = ViewAxis::parse(&config.view.default_axis) {
            camera.set_view_axis(axis);
        }

        // Apply the config base color to meshes that have no authored color
        // (fixes "mesh file colors are never used": authored colors win).
        let mut scene = scene;
        for m in &mut scene.meshes {
            if m.original_color.is_none() {
                m.color = config.material.base_color;
            }
        }

        let theme = resolve_theme(&config.ui.sidepanel.theme);
        let wireframe_color = if theme.is_dark() {
            config.wireframe.color_dark_bg
        } else {
            config.wireframe.color_light_bg
        };
        let rasterizer = SoftwareRasterizer {
            options: Options {
                lighting: Lighting {
                    key_intensity: config.lighting.key_light_intensity,
                    fill_intensity: config.lighting.fill_light_intensity,
                    fill_dir: Vec3::new(-0.5, 0.3, -1.0).normalize(),
                    rim_intensity: config.lighting.rim_light_intensity,
                    rim_dir: Vec3::new(0.5, 0.2, 1.0).normalize(),
                    ambient: config.scene.ambient_light,
                },
                shininess: (1.0 - config.material.roughness_factor).clamp(0.0, 1.0) * 100.0,
                wireframe_thickness: config.wireframe.default_thickness,
                wireframe_color,
            },
        };
        let wireframe_thickness = config.wireframe.default_thickness;
        let show_scene_info = config.ui.sidepanel.show_scene_info;
        let selected = scene
            .meshes
            .iter()
            .enumerate()
            .min_by_key(|(_, mesh)| mesh.name.to_lowercase())
            .map(|(index, _)| index)
            .unwrap_or(0);
        let filtered_indices =
            filter_mesh_indices(&scene, "").expect("an empty mesh filter is always valid");
        let base_orientation = camera.orientation;
        let base_up = Vec3::from(
            config
                .view
                .up_vectors
                .first()
                .copied()
                .unwrap_or([0.0, 1.0, 0.0]),
        )
        .normalize_or(Vec3::Y);
        Self {
            scene,
            camera,
            config,
            rasterizer,
            selected,
            marked: BTreeSet::new(),
            mesh_filter: String::new(),
            filtered_indices,
            deleted: Vec::new(),
            show_sidepanel: true,
            show_scene_info,
            modal: None,
            animation: None,
            wireframe_thickness,
            light_scale: 1.0,
            theme,
            status_message: None,
            aspect: -1.0, // sentinel: unset until set_aspect() before first frame
            base_orientation,
            base_up,
            pending_zoom: None,
            pending_distance: None,
            auto_zoom: true,
            dirty: true,
            render_dirty: true,
        }
    }

    /// Re-fit the initial framing to the real viewport aspect (width/height),
    /// applied once before the first frame so the mesh fills the actual
    /// viewport instead of a square estimate. No-op after that, so later
    /// terminal resizes don't reset the user's zoom.
    pub fn set_aspect(&mut self, aspect: f32) {
        if self.aspect >= 0.0 {
            return; // already applied (or set); only the first call reframes
        }
        let aspect = if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            1.0
        };
        self.aspect = aspect;
        if let Some((min, max)) = self.scene.visible_bounds() {
            self.camera.reframe_bounds_aspect(
                min,
                max,
                self.config.camera.distance_padding,
                aspect,
            );
        }
        // The fit resets distance/ortho_scale, so deferred scene zoom and
        // distance must be re-applied on top of it.
        if let Some(zoom) = self.pending_zoom.take() {
            self.camera.zoom(zoom);
        }
        if let Some(distance) = self.pending_distance.take() {
            self.camera.distance = distance.max(1e-3);
        }
        self.render_dirty = true;
    }

    /// Apply scene zoom/distance AFTER the one-time auto-fit (the fit would
    /// cancel them). Applies immediately when the fit already happened;
    /// otherwise deferred until `set_aspect` runs (TUI scene open, where the
    /// terminal aspect is unknown until the first frame).
    pub fn apply_post_fit_camera(&mut self, zoom: Option<f32>, distance: Option<f32>) {
        if self.aspect >= 0.0 {
            if let Some(zoom) = zoom {
                self.camera.zoom(zoom);
            }
            if let Some(distance) = distance {
                self.camera.distance = distance.max(1e-3);
            }
        } else {
            self.pending_zoom = zoom;
            self.pending_distance = distance;
        }
    }

    /// Re-resolve the theme (omarchy live-reload) and restyle without a
    /// restart.
    pub fn reload_theme(&mut self) {
        self.theme = resolve_theme(&self.config.ui.sidepanel.theme);
        self.rasterizer.options.wireframe_color = if self.theme.is_dark() {
            self.config.wireframe.color_dark_bg
        } else {
            self.config.wireframe.color_light_bg
        };
        self.dirty = true;
        self.render_dirty = true;
    }

    /// Render the current state to a pixel frame. Returns `None` when every
    /// mesh is hidden (never NaN garbage, unlike the Python version).
    pub fn render_frame(&mut self, width: u32, height: u32) -> Option<Frame> {
        self.update_light_dirs();
        self.rasterizer.options.wireframe_thickness = self.wireframe_thickness;
        self.rasterizer
            .render(&self.scene, &self.camera, width, height)
    }

    /// Directional 3-point lights anchored to the camera, azimuth/elevation
    /// offsets from config. One consistent eye→target convention.
    fn update_light_dirs(&mut self) {
        let fwd = (self.camera.position() - self.camera.target).normalize_or(Vec3::Z);
        // Camera-local offsets: azimuth around the camera up axis, elevation
        // around the camera right axis — stable from every view, unlike a
        // global spherical offset which degenerates when looking along ±Z.
        let lc = &self.config.lighting;
        let l = &mut self.rasterizer.options.lighting;
        l.fill_dir = camera_light_offset(
            fwd,
            self.camera.up(),
            lc.fill_light_azimuth,
            lc.fill_light_elevation,
        );
        l.rim_dir = camera_light_offset(
            fwd,
            self.camera.up(),
            lc.rim_light_azimuth,
            lc.rim_light_elevation,
        );
        l.key_intensity = lc.key_light_intensity * self.light_scale;
        l.fill_intensity = lc.fill_light_intensity * self.light_scale;
        l.rim_intensity = lc.rim_light_intensity * self.light_scale;
    }

    fn key_to_string(ev: &KeyEvent) -> Option<String> {
        if ev.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char(c) = ev.code {
                return Some(format!("ctrl+{c}"));
            }
        }
        Some(match ev.code {
            KeyCode::Char(' ') => "space".into(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Enter => "enter".into(),
            KeyCode::Esc => "escape".into(),
            KeyCode::Up => "up".into(),
            KeyCode::Down => "down".into(),
            KeyCode::Left => "left".into(),
            KeyCode::Right => "right".into(),
            KeyCode::Tab => "tab".into(),
            KeyCode::Backspace => "backspace".into(),
            _ => return None,
        })
    }

    /// Apply a key. Returns true if the app should quit.
    fn handle_key(&mut self, key: &str) -> bool {
        if key == "ctrl+c" {
            return true;
        }
        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }
        if key == "?" {
            self.modal = Some(Modal::CommandPalette {
                query: String::new(),
                selected: 0,
            });
            self.dirty = true;
            return false;
        }
        let Some(action) = self.action_for_key(key) else {
            return false;
        };
        self.execute_action(&action)
    }

    fn action_for_key(&self, key: &str) -> Option<String> {
        self.config.keybindings.iter().find_map(|(action, value)| {
            let matches = match value {
                toml::Value::String(s) => s == key,
                toml::Value::Array(a) => a.iter().any(|v| v.as_str() == Some(key)),
                _ => false,
            };
            matches.then_some(action.clone())
        })
    }

    fn execute_action(&mut self, action: &str) -> bool {
        self.status_message = None;
        let oc = self.config.orbital_camera.clone();
        let speed = oc.movement_speed;
        let fast = speed * oc.movement_speed_fast_multiplier;
        let mut rerender = true;
        match action {
            "quit" => return true,
            "toggle_sidepanel" => self.show_sidepanel = !self.show_sidepanel,
            "toggle_scene_info" => {
                self.show_scene_info = !self.show_scene_info;
                rerender = false;
            }
            "orbit_left" => self.camera.orbit(-speed, 0.0),
            "orbit_right" => self.camera.orbit(speed, 0.0),
            "orbit_up" => self.camera.orbit(0.0, -speed),
            "orbit_down" => self.camera.orbit(0.0, speed),
            "orbit_left_fast" => self.camera.orbit(-fast, 0.0),
            "orbit_right_fast" => self.camera.orbit(fast, 0.0),
            "orbit_up_fast" => self.camera.orbit(0.0, -fast),
            "orbit_down_fast" => self.camera.orbit(0.0, fast),
            "zoom_in" => self.camera.zoom(oc.zoom_in_factor),
            "zoom_out" => self.camera.zoom(oc.zoom_out_factor),
            "toggle_auto_zoom" => {
                self.auto_zoom = !self.auto_zoom;
                self.status_message = Some(if self.auto_zoom {
                    "auto-zoom on (reframes when meshes change)".into()
                } else {
                    "auto-zoom off (camera distance fixed)".into()
                });
                rerender = false;
            }
            "reset_orbital" => self.reset_camera(),
            "view_minus_x" => self.camera.set_view_axis(ViewAxis::NegX),
            "view_plus_x" => self.camera.set_view_axis(ViewAxis::PosX),
            "view_minus_y" => self.camera.set_view_axis(ViewAxis::NegY),
            "view_plus_y" => self.camera.set_view_axis(ViewAxis::PosY),
            "view_minus_z" => self.camera.set_view_axis(ViewAxis::NegZ),
            "view_plus_z" => self.camera.set_view_axis(ViewAxis::PosZ),
            "camera_orthographic" => self.set_camera_kind(CameraKind::Orthographic),
            "camera_perspective" => self.set_camera_kind(CameraKind::Perspective),
            "up_vector_next" => self
                .camera
                .cycle_up(&self.config.view.up_vectors.clone(), true),
            "up_vector_prev" => self
                .camera
                .cycle_up(&self.config.view.up_vectors.clone(), false),
            "wireframe_off" => self.wireframe_thickness = 0.0,
            "wireframe_increase" => {
                self.wireframe_thickness = if self.wireframe_thickness <= 0.0 {
                    1.0
                } else {
                    (self.wireframe_thickness + 0.5).min(10.0)
                }
            }
            "light_decrease" => self.light_scale = (self.light_scale - 0.1).max(0.0),
            "light_increase" => self.light_scale = (self.light_scale + 0.1).min(4.0),
            "mesh_hide" => {
                let count = self.target_indices().len();
                self.set_targets_visible(false);
                self.status_message = Some(format!("hid {count} mesh(es)"));
            }
            "mesh_show" => {
                let count = self.target_indices().len();
                self.set_targets_visible(true);
                self.status_message = Some(format!("showed {count} mesh(es)"));
            }
            "reset_single_mesh" => {
                let count = self.target_indices().len();
                self.reset_targets();
                self.status_message = Some(format!("reset {count} mesh(es)"));
            }
            "reset_all_meshes" => self.reset_all(),
            "mesh_color_next" => {
                let count = self.target_indices().len();
                self.cycle_color(1);
                self.status_message = Some(format!("recolored {count} mesh(es)"));
            }
            "mesh_color_prev" => {
                let count = self.target_indices().len();
                self.cycle_color(-1);
                self.status_message = Some(format!("recolored {count} mesh(es)"));
            }
            "mesh_alpha_up" => {
                let count = self.target_indices().len();
                self.cycle_alpha(0.25);
                self.status_message = Some(format!("opacity up for {count} mesh(es)"));
            }
            "mesh_alpha_down" => {
                let count = self.target_indices().len();
                self.cycle_alpha(-0.25);
                self.status_message = Some(format!("opacity down for {count} mesh(es)"));
            }
            "mesh_custom_color" => {
                self.modal = Some(Modal::HexColor {
                    input: String::new(),
                    error: None,
                });
                rerender = false;
            }
            "mesh_toggle_mark" => {
                self.toggle_selected_mark();
                rerender = false;
            }
            "mesh_select_all" => {
                self.select_all_filtered();
                rerender = false;
            }
            "mesh_select_none" | "mesh_clear_marks" => {
                self.select_none();
                rerender = false;
            }
            "mesh_filter" => {
                self.modal = Some(Modal::MeshFilter {
                    input: self.mesh_filter.clone(),
                    error: None,
                });
                rerender = false;
            }
            "mesh_clear_filter" => {
                self.set_mesh_filter("")
                    .expect("an empty mesh filter is always valid");
                self.status_message = Some(format!(
                    "filter cleared ({} meshes)",
                    self.filtered_indices.len()
                ));
                rerender = false;
            }
            "mesh_open" => {
                self.modal = Some(Modal::OpenPath {
                    input: String::new(),
                    error: None,
                });
                rerender = false;
            }
            "mesh_delete" => {
                let count = self.delete_targets();
                self.status_message = Some(if count == 0 {
                    "nothing to delete".into()
                } else {
                    let undo_key = key_label(
                        &self
                            .config
                            .key("mesh_undo_delete")
                            .unwrap_or_else(|| "-".into()),
                    );
                    format!("deleted {count} mesh(es) ({undo_key} to restore)")
                });
                rerender = false;
            }
            "mesh_undo_delete" => {
                let count = self.restore_deleted();
                self.status_message = Some(if count == 0 {
                    "nothing to restore".into()
                } else {
                    format!("restored {count} mesh(es)")
                });
                rerender = false;
            }
            "animation_start" => {
                self.modal = Some(Modal::AnimationDelay {
                    input: String::new(),
                    error: None,
                });
                rerender = false;
            }
            "animation_stop" => {
                if self.animation.take().is_some() {
                    self.render_dirty = true;
                }
                self.status_message = Some("animation stopped".into());
                rerender = self.render_dirty;
            }
            "screenshot" => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let path = std::path::PathBuf::from(format!("meshtui_{ts}.png"));
                self.status_message = Some(match save_screenshot(self, &path, 1600, 1200) {
                    Ok(()) => format!("saved {}", path.display()),
                    Err(error) => format!("screenshot failed: {error}"),
                });
                rerender = false;
            }
            "sidepanel_move_up" => {
                self.move_selection(-1);
                rerender = false;
            }
            "sidepanel_move_down" => {
                self.move_selection(1);
                rerender = false;
            }
            _ => return false, // unknown action: no redraw
        }
        self.dirty = true;
        self.render_dirty |= rerender;
        false
    }

    fn handle_modal_key(&mut self, key: &str) -> bool {
        let Some(mut modal) = self.modal.take() else {
            return false;
        };
        let mut keep_open = true;
        let mut action_to_run = None;
        match &mut modal {
            Modal::CommandPalette { query, selected } => match key {
                "escape" => keep_open = false,
                "up" | "down" => {
                    let count = self.palette_matches(query).len();
                    if count > 0 {
                        let delta = if key == "up" { count - 1 } else { 1 };
                        *selected = (*selected + delta) % count;
                    }
                }
                "enter" => {
                    if let Some(command) = self.palette_matches(query).get(*selected) {
                        action_to_run = Some(command.action);
                        keep_open = false;
                    }
                }
                "backspace" => {
                    query.pop();
                    *selected = 0;
                }
                _ => {
                    if let Some(ch) = input_char(key) {
                        query.push(ch);
                        *selected = 0;
                    }
                }
            },
            Modal::HexColor { input, error } => match key {
                "escape" => keep_open = false,
                "enter" => match parse_hex_color(input) {
                    Ok(color) => {
                        self.set_targets_color(color);
                        self.render_dirty = true;
                        self.status_message = Some(format!(
                            "updated color for {} mesh(es)",
                            self.target_indices().len()
                        ));
                        keep_open = false;
                    }
                    Err(message) => *error = Some(message.into()),
                },
                "backspace" => {
                    input.pop();
                    *error = None;
                }
                _ => {
                    if let Some(ch) = input_char(key) {
                        input.push(ch);
                        *error = None;
                    }
                }
            },
            Modal::AnimationDelay { input, error } => match key {
                "escape" => keep_open = false,
                "enter" => {
                    let interval_ms = if input.trim().is_empty() {
                        Some(self.config.ui.animation.default_interval_ms)
                    } else {
                        input.trim().parse::<u64>().ok()
                    };
                    match interval_ms {
                        Some(interval_ms) if (1..=10_000).contains(&interval_ms) => {
                            modal = Modal::AnimationDirection { interval_ms };
                        }
                        _ => *error = Some("Enter a delay from 1 to 10000 ms".into()),
                    }
                }
                "backspace" => {
                    input.pop();
                    *error = None;
                }
                _ => {
                    if key.chars().all(|ch| ch.is_ascii_digit()) {
                        input.push_str(key);
                        *error = None;
                    }
                }
            },
            Modal::AnimationDirection { interval_ms } => {
                if key == "escape" || self.action_for_key(key).as_deref() == Some("animation_stop")
                {
                    keep_open = false;
                } else if let Some(action) = self.action_for_key(key) {
                    let action = match action.as_str() {
                        "orbit_left" => Some("orbit_left"),
                        "orbit_right" => Some("orbit_right"),
                        "orbit_up" => Some("orbit_up"),
                        "orbit_down" => Some("orbit_down"),
                        _ => None,
                    };
                    if let Some(action) = action {
                        self.start_animation(action, *interval_ms);
                        keep_open = false;
                    }
                }
            }
            Modal::MeshFilter { input, error } => match key {
                "escape" => keep_open = false,
                "enter" => match self.set_mesh_filter(input) {
                    Ok(()) => {
                        self.status_message = Some(format!(
                            "{} mesh(es) match the filter",
                            self.filtered_indices.len()
                        ));
                        keep_open = false;
                    }
                    Err(message) => *error = Some(message),
                },
                "backspace" => {
                    input.pop();
                    *error = None;
                }
                _ => {
                    if let Some(ch) = input_char(key) {
                        input.push(ch);
                        *error = None;
                    }
                }
            },
            Modal::OpenPath { input, error } => match key {
                "escape" => keep_open = false,
                "enter" => {
                    let path = expand_home(input.trim());
                    match load_path(std::path::Path::new(&path)) {
                        Ok(meshes) => {
                            let count = self.add_meshes(meshes);
                            self.status_message = Some(format!("added {count} mesh(es)"));
                            keep_open = false;
                        }
                        Err(load_error) => *error = Some(load_error.to_string()),
                    }
                }
                "backspace" => {
                    input.pop();
                    *error = None;
                }
                "tab" => {
                    let (completed, _) = complete_path(input);
                    *input = completed;
                    *error = None;
                }
                _ => {
                    if let Some(ch) = input_char(key) {
                        input.push(ch);
                        *error = None;
                    }
                }
            },
        }
        if keep_open {
            self.modal = Some(modal);
        }
        self.dirty = true;
        action_to_run.is_some_and(|action| self.execute_action(action))
    }

    fn palette_matches(&self, query: &str) -> Vec<&'static CommandSpec> {
        let threshold = self.config.ui.command_palette.fuzzy_match_threshold;
        COMMANDS
            .iter()
            .filter(|command| {
                let key = self.config.key(command.action).unwrap_or_default();
                fuzzy_score(
                    query,
                    &format!("{} {} {}", command.section, key, command.description),
                ) >= threshold
            })
            .collect()
    }

    fn mesh_indices(&self) -> &[usize] {
        &self.filtered_indices
    }

    fn set_mesh_filter(&mut self, filter: &str) -> Result<(), String> {
        let filter = filter.trim();
        let indices = filter_mesh_indices(&self.scene, filter)?;
        self.mesh_filter = filter.to_string();
        self.filtered_indices = indices;
        self.ensure_selected_in_filter();
        Ok(())
    }

    fn ensure_selected_in_filter(&mut self) {
        if !self.filtered_indices.contains(&self.selected) {
            if let Some(&first) = self.filtered_indices.first() {
                self.selected = first;
            }
        }
    }

    fn target_indices(&self) -> Vec<usize> {
        let filtered_marks: Vec<usize> = self
            .filtered_indices
            .iter()
            .copied()
            .filter(|index| self.marked.contains(index))
            .collect();
        if !filtered_marks.is_empty() {
            return filtered_marks;
        }
        self.filtered_indices
            .contains(&self.selected)
            .then_some(self.selected)
            .into_iter()
            .collect()
    }

    fn set_targets_visible(&mut self, visible: bool) {
        for index in self.target_indices() {
            if let Some(mesh) = self.scene.meshes.get_mut(index) {
                mesh.visible = visible;
            }
        }
        self.frame_visible_meshes();
    }

    /// Reset restores the mesh's *authored* color, not hardcoded white.
    fn reset_targets(&mut self) {
        let default_color = self.config.material.base_color;
        for index in self.target_indices() {
            if let Some(mesh) = self.scene.meshes.get_mut(index) {
                mesh.color = mesh.original_color.unwrap_or(default_color);
                mesh.visible = true;
            }
        }
        self.frame_visible_meshes();
    }

    /// Scale all three light intensities (like the TUI `i`/`I`).
    pub fn set_light_scale(&mut self, scale: f32) {
        self.light_scale = scale.clamp(0.0, 4.0);
    }

    /// Current light intensity scale.
    pub fn light_scale(&self) -> f32 {
        self.light_scale
    }

    /// Set the wireframe overlay thickness in pixels (0 disables).
    pub fn set_wireframe_thickness(&mut self, thickness: f32) {
        self.wireframe_thickness = thickness.clamp(0.0, 10.0);
    }

    /// Current wireframe overlay thickness in pixels.
    pub fn wireframe_thickness(&self) -> f32 {
        self.wireframe_thickness
    }

    /// Recenter and refit zoom to currently visible meshes, keeping orbit.
    /// No-op when auto-zoom is toggled off, so the camera distance stays
    /// constant while meshes are hidden/shown (steady animation frames).
    fn frame_visible_meshes(&mut self) {
        if !self.auto_zoom {
            self.render_dirty = true; // visibility changed; framing must not
            return;
        }
        self.frame_visible_meshes_forced();
    }

    /// Refit regardless of the auto-zoom toggle — for deliberate user
    /// actions that require it (projection switch, explicit camera reset).
    fn frame_visible_meshes_forced(&mut self) {
        if let Some((min, max)) = self.scene.visible_bounds() {
            self.camera.reframe_bounds_aspect(
                min,
                max,
                self.config.camera.distance_padding,
                self.aspect.max(1.0),
            );
        }
    }

    /// Switch ortho/perspective and re-fit the zoom. The two projections frame
    /// the same scene at different distances (ortho is depth-independent,
    /// perspective fits the frustum), so keeping the old distance would clip
    /// or over-zoom. The orbit (target, orientation) is preserved. Always
    /// refits, even with auto-zoom off — the switch is deliberate.
    fn set_camera_kind(&mut self, kind: CameraKind) {
        if self.camera.kind == kind {
            return;
        }
        self.camera.kind = kind;
        self.frame_visible_meshes_forced();
    }

    fn reset_all(&mut self) {
        let default_color = self.config.material.base_color;
        for m in &mut self.scene.meshes {
            m.color = m.original_color.unwrap_or(default_color);
            m.visible = true;
        }
        self.reset_camera();
    }

    fn reset_camera(&mut self) {
        if let Some((min, max)) = self.scene.visible_bounds() {
            let kind = self.camera.kind;
            let fov = self.camera.fov_degrees;
            let up = self.camera.up();
            self.camera = Camera::frame_bounds_aspect(
                min,
                max,
                kind,
                fov,
                self.config.camera.distance_padding,
                self.aspect.max(1.0),
            );
            self.camera.set_up(up);
        }
    }

    /// Cycle per-mesh opacity (t/T). Stepping past opaque wraps to fully
    /// transparent and back so repeated presses toggle through the levels.
    fn cycle_alpha(&mut self, step: f32) {
        for index in self.target_indices() {
            if let Some(mesh) = self.scene.meshes.get_mut(index) {
                let a = mesh.color[3];
                // Snap to clean quarter steps, then advance and wrap 0..=1.
                let level =
                    ((a * 4.0).round() as i32 + if step > 0.0 { 1 } else { -1 }).rem_euclid(5);
                mesh.color[3] = level as f32 / 4.0;
            }
        }
        self.render_dirty = true;
    }

    fn cycle_color(&mut self, dir: i32) {
        let palette = self.config.ui.sidepanel.color_palette.clone();
        if palette.is_empty() {
            return;
        }
        let targets = self.target_indices();
        let Some(&first) = targets.first() else {
            return;
        };
        let current = self
            .scene
            .meshes
            .get(first)
            .map(|mesh| mesh.color)
            .unwrap_or(palette[0]);
        let palette_index = palette
            .iter()
            .position(|color| *color == current)
            .map(|index| index as i32)
            .unwrap_or(0);
        let count = palette.len() as i32;
        let next = palette[((palette_index + dir).rem_euclid(count)) as usize];
        for index in targets {
            if let Some(mesh) = self.scene.meshes.get_mut(index) {
                mesh.color = next;
            }
        }
    }

    fn set_targets_color(&mut self, color: [f32; 4]) {
        for index in self.target_indices() {
            if let Some(mesh) = self.scene.meshes.get_mut(index) {
                mesh.color = color;
            }
        }
    }

    fn toggle_selected_mark(&mut self) {
        if !self.marked.remove(&self.selected) && self.filtered_indices.contains(&self.selected) {
            self.marked.insert(self.selected);
        }
    }

    fn select_all_filtered(&mut self) {
        self.marked = self.filtered_indices.iter().copied().collect();
        self.status_message = Some(format!("selected {} mesh(es)", self.marked.len()));
    }

    fn select_none(&mut self) {
        let count = self.marked.len();
        self.marked.clear();
        self.status_message = Some(format!("cleared {count} selected mesh(es)"));
    }

    /// Remove the selected/marked meshes from the scene, keeping them on the
    /// undo stack. Returns how many were deleted.
    fn delete_targets(&mut self) -> usize {
        let mut targets = self.target_indices();
        targets.sort_unstable();
        targets.dedup();
        if targets.is_empty() {
            return 0;
        }
        let selected_deleted = targets.contains(&self.selected);
        let mut entries = Vec::with_capacity(targets.len());
        let mut marked = BTreeSet::new();
        for &index in targets.iter().rev() {
            let mesh = self.scene.meshes.remove(index);
            if self.marked.remove(&index) {
                marked.insert(index);
            }
            entries.push((index, mesh));
        }
        entries.reverse();
        // Shift the surviving marks down past the removed indices.
        self.marked = self
            .marked
            .iter()
            .map(|&m| m - targets.iter().filter(|&&t| t < m).count())
            .collect();
        if !selected_deleted {
            self.selected -= targets.iter().filter(|&&t| t < self.selected).count();
        }
        let count = entries.len();
        if self.deleted.len() >= 32 {
            self.deleted.remove(0);
        }
        self.deleted.push(DeletedMeshes { entries, marked });
        let filter = self.mesh_filter.clone();
        self.set_mesh_filter(&filter)
            .expect("the active filter stays valid");
        if selected_deleted {
            self.selected = self.filtered_indices.first().copied().unwrap_or(0);
        }
        self.frame_visible_meshes();
        self.render_dirty = true;
        count
    }

    /// Restore the most recently deleted meshes (LIFO). Returns how many
    /// came back.
    fn restore_deleted(&mut self) -> usize {
        let Some(entry) = self.deleted.pop() else {
            return 0;
        };
        let count = entry.entries.len();
        let mut first_restored = None;
        for (index, mesh) in entry.entries {
            let at = index.min(self.scene.meshes.len());
            self.scene.meshes.insert(at, mesh);
            // Marks at/after the insertion point shift up.
            self.marked = self
                .marked
                .iter()
                .map(|&m| if m >= at { m + 1 } else { m })
                .collect();
            if first_restored.is_none() {
                first_restored = Some(at);
            }
        }
        // Reinsertion at the original indices makes the stored marks valid.
        self.marked.extend(entry.marked);
        if let Some(at) = first_restored {
            self.selected = at;
        }
        let filter = self.mesh_filter.clone();
        self.set_mesh_filter(&filter)
            .expect("the active filter stays valid");
        self.frame_visible_meshes();
        self.render_dirty = true;
        count
    }

    /// Append loaded meshes to the scene, re-apply the filter, and reframe.
    /// Returns how many were added.
    fn add_meshes(&mut self, meshes: Vec<Mesh>) -> usize {
        let first_new = self.scene.meshes.len();
        let count = meshes.len();
        if count == 0 {
            return 0;
        }
        self.scene.meshes.extend(meshes);
        self.selected = first_new;
        let filter = self.mesh_filter.clone();
        self.set_mesh_filter(&filter)
            .expect("the active filter stays valid");
        self.frame_visible_meshes();
        self.render_dirty = true;
        count
    }

    fn move_selection(&mut self, dir: i32) {
        let indices = &self.filtered_indices;
        if indices.is_empty() {
            return;
        }
        let position = indices
            .iter()
            .position(|&index| index == self.selected)
            .unwrap_or(0) as i32;
        self.selected = indices[(position + dir).rem_euclid(indices.len() as i32) as usize];
    }

    fn start_animation(&mut self, action: &'static str, interval_ms: u64) {
        let interval = Duration::from_millis(interval_ms);
        let now = Instant::now();
        let render_interval =
            Duration::from_secs_f64(1.0 / self.config.ui.animation.animation_fps as f64);
        self.animation = Some(AnimationState {
            action,
            interval,
            next_step: now + interval,
            render_interval,
            next_render: now,
            pending_render: false,
        });
        self.status_message = Some(format!("animation {action} every {interval_ms} ms"));
    }

    fn advance_animation(&mut self, now: Instant) {
        let Some(animation) = &mut self.animation else {
            return;
        };
        if now >= animation.next_step {
            let speed = self.config.orbital_camera.movement_speed;
            let overdue = now.duration_since(animation.next_step);
            let missed = overdue.as_nanos() / animation.interval.as_nanos();
            let tick_count = (missed.min((u32::MAX - 1) as u128) as u32) + 1;
            let delta = speed * tick_count as f32;
            match animation.action {
                "orbit_left" => self.camera.orbit(-delta, 0.0),
                "orbit_right" => self.camera.orbit(delta, 0.0),
                "orbit_up" => self.camera.orbit(0.0, -delta),
                "orbit_down" => self.camera.orbit(0.0, delta),
                _ => {}
            }
            animation.next_step += animation.interval * tick_count;
            animation.pending_render = true;
        }
        if self.config.ui.animation.render_on_animate
            && animation.pending_render
            && now >= animation.next_render
        {
            self.render_dirty = true;
            self.dirty = true;
            animation.pending_render = false;
            animation.next_render = now + animation.render_interval;
        }
    }

    fn wait_duration(&self, theme_watcher: bool, now: Instant) -> Duration {
        let mut wait = if theme_watcher {
            Duration::from_millis(250)
        } else {
            Duration::from_secs(3600)
        };
        if let Some(animation) = &self.animation {
            wait = wait.min(animation.next_step.saturating_duration_since(now));
            if self.config.ui.animation.render_on_animate && animation.pending_render {
                wait = wait.min(animation.next_render.saturating_duration_since(now));
            }
        }
        wait
    }

    fn draw_ui(&mut self, f: &mut TuiFrame) {
        let theme = self.theme;
        let panel_style = Style::default().fg(theme.foreground).bg(theme.background);
        let area = f.area();
        let (viewport, sidepanel) = if self.show_sidepanel {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(10), Constraint::Length(28)])
                .split(area);
            (chunks[0], Some(chunks[1]))
        } else {
            (area, None)
        };
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(viewport);
        let status = rows[1];

        let kind = match self.camera.kind {
            CameraKind::Perspective => "persp",
            CameraKind::Orthographic => "ortho",
        };
        let status_text = self.status_message.clone().unwrap_or_else(|| {
            let animation = self.animation.as_ref().map_or("", |_| " | animating");
            let filter = if self.mesh_filter.is_empty() {
                String::new()
            } else {
                format!(" | filter:{:?}", self.mesh_filter)
            };
            let fit = if self.auto_zoom { "" } else { " | fit:manual" };
            format!(
                " {kind} | wf:{:.1} | light:{:.1}x | marked:{}{filter}{animation}{fit} | ? commands | q quit",
                self.wireframe_thickness, self.light_scale, self.marked.len(),
            )
        });
        f.render_widget(
            Paragraph::new(status_text)
                .style(Style::default().fg(theme.muted).bg(theme.background)),
            status,
        );

        if let Some(sp) = sidepanel {
            let sp_rows = if self.show_scene_info {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(3),
                        Constraint::Length(2),
                        Constraint::Length(9),
                    ])
                    .split(sp)
                    .to_vec()
            } else {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(3), Constraint::Length(2)])
                    .split(sp)
                    .to_vec()
            };
            let mesh_indices = self.mesh_indices();
            let items: Vec<ListItem> = mesh_indices
                .iter()
                .map(|&index| {
                    let mesh = &self.scene.meshes[index];
                    let vis = if mesh.visible { "●" } else { "○" };
                    let marked = if self.marked.contains(&index) {
                        "✓"
                    } else {
                        " "
                    };
                    let color = TColor::Rgb(
                        (mesh.color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                        (mesh.color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                        (mesh.color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
                    );
                    // Style the selected row per-span instead of via
                    // List::highlight_style, which is patched over the whole
                    // row after rendering and would repaint the color dot
                    // with the selection foreground.
                    let is_selected = index == self.selected;
                    let row_style = if is_selected {
                        Style::default()
                            .fg(theme.selection_foreground)
                            .bg(theme.selection)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        panel_style
                    };
                    let dot_bg = if is_selected {
                        theme.selection
                    } else {
                        theme.background
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{marked} {vis} "),
                            Style::default().fg(color).bg(dot_bg),
                        ),
                        Span::styled(truncate(&mesh.name, 19), row_style),
                    ]))
                    .style(row_style)
                })
                .collect();
            let count = format!("{}/{}", mesh_indices.len(), self.scene.meshes.len());
            let title = if self.mesh_filter.is_empty() {
                format!("Meshes {count}")
            } else {
                truncate(&format!("Meshes {count}: {}", self.mesh_filter), 24)
            };
            let list = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .border_style(Style::default().fg(theme.accent))
                        .style(panel_style),
                )
                .style(panel_style)
                .highlight_symbol("> ");
            let mut state = ListState::default();
            if let Some(position) = mesh_indices
                .iter()
                .position(|&index| index == self.selected)
            {
                state.select(Some(position));
            }
            f.render_stateful_widget(list, sp_rows[0], &mut state);

            let all_key = key_label(
                &self
                    .config
                    .key("mesh_select_all")
                    .unwrap_or_else(|| "-".into()),
            );
            let none_key = key_label(
                &self
                    .config
                    .key("mesh_select_none")
                    .unwrap_or_else(|| "-".into()),
            );
            let mark_key = key_label(
                &self
                    .config
                    .key("mesh_toggle_mark")
                    .unwrap_or_else(|| "-".into()),
            );
            let filter_key =
                key_label(&self.config.key("mesh_filter").unwrap_or_else(|| "-".into()));
            let clear_key = key_label(
                &self
                    .config
                    .key("mesh_clear_filter")
                    .unwrap_or_else(|| "-".into()),
            );
            f.render_widget(
                Paragraph::new(format!(
                    " {all_key} all | {none_key} none | {mark_key} mark\n {filter_key} filter | {clear_key} clear"
                ))
                .style(Style::default().fg(theme.muted).bg(theme.background)),
                sp_rows[1],
            );

            if self.show_scene_info && sp_rows.len() > 2 {
                let camera = self.camera.position();
                let info = Paragraph::new(format!(
                    "visible: {}/{}\nverts:   {}\nfaces:   {}\ncam: ({:.1},{:.1},{:.1})\n{} {:.1}°\nup: ({:.0},{:.0},{:.0})",
                    self.scene.visible_count(),
                    self.scene.meshes.len(),
                    self.scene.total_vertices(),
                    self.scene.total_triangles(),
                    camera.x,
                    camera.y,
                    camera.z,
                    kind,
                    self.camera.fov_degrees,
                    self.camera.up().x,
                    self.camera.up().y,
                    self.camera.up().z,
                ))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Scene")
                        .border_style(Style::default().fg(theme.accent))
                        .style(panel_style),
                )
                .style(panel_style);
                f.render_widget(info, sp_rows[2]);
            }
        }

        if let Some(modal) = &self.modal {
            self.draw_modal(f, modal);
        }
    }

    fn draw_modal(&self, f: &mut TuiFrame, modal: &Modal) {
        match modal {
            Modal::CommandPalette { query, selected } => {
                draw_command_palette(f, self, query, *selected)
            }
            Modal::HexColor { input, error } => draw_input_modal(
                f,
                self.theme,
                " Mesh Color ",
                "Enter #RRGGBB",
                input,
                error.as_deref(),
            ),
            Modal::AnimationDelay { input, error } => draw_input_modal(
                f,
                self.theme,
                " Animation ",
                "Delay in ms (empty uses configured default)",
                input,
                error.as_deref(),
            ),
            Modal::AnimationDirection { interval_ms } => {
                draw_animation_direction(f, self, *interval_ms)
            }
            Modal::MeshFilter { input, error } => draw_input_modal(
                f,
                self.theme,
                " Filter Meshes ",
                "Substring or re:<regex>; prefix ! to invert",
                input,
                error.as_deref(),
            ),
            Modal::OpenPath { input, error } => {
                draw_open_modal(f, self.theme, input, error.as_deref())
            }
        }
    }
}

fn filter_mesh_indices(scene: &Scene, filter: &str) -> Result<Vec<usize>, String> {
    let filter = filter.trim();
    let (invert, pattern) = match filter.strip_prefix('!') {
        Some(rest) => (true, rest.trim()),
        None => (false, filter),
    };
    let regex = pattern
        .strip_prefix("re:")
        .map(|pattern| {
            RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
                .map_err(|error| format!("Invalid regex: {error}"))
        })
        .transpose()?;
    let literal = regex.is_none().then(|| pattern.to_lowercase());
    let mut matches: Vec<(usize, String)> = scene
        .meshes
        .iter()
        .enumerate()
        .filter_map(|(index, mesh)| {
            let sort_name = mesh.name.to_lowercase();
            let is_match = regex
                .as_ref()
                .is_some_and(|regex| regex.is_match(&mesh.name))
                || literal
                    .as_ref()
                    .is_some_and(|literal| literal.is_empty() || sort_name.contains(literal));
            (is_match != invert).then_some((index, sort_name))
        })
        .collect();
    matches.sort_by(|(a_index, a_name), (b_index, b_name)| {
        a_name.cmp(b_name).then_with(|| a_index.cmp(b_index))
    });
    Ok(matches.into_iter().map(|(index, _)| index).collect())
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

fn key_label(key: &str) -> String {
    if let Some(key) = key.strip_prefix("ctrl+") {
        format!("^{}", key.to_uppercase())
    } else if key == "space" {
        "Spc".into()
    } else {
        key.into()
    }
}

fn draw_command_palette(f: &mut TuiFrame, app: &App, query: &str, selected: usize) {
    let commands = app.palette_matches(query);
    let max_items = app.config.ui.command_palette.max_visible_items;
    let start = selected
        .saturating_sub(max_items / 2)
        .min(commands.len().saturating_sub(max_items));
    let end = (start + max_items).min(commands.len());
    let visible = &commands[start..end];
    let section_count = visible
        .iter()
        .map(|command| command.section)
        .fold(Vec::<&str>::new(), |mut sections, section| {
            if sections.last().copied() != Some(section) {
                sections.push(section);
            }
            sections
        })
        .len();
    let height = (visible.len() + section_count + 6) as u16;
    let area = centered_rect_cells(70, height.min(f.area().height.saturating_sub(2)), f.area());
    let theme = app.theme;
    let base_style = Style::default().fg(theme.foreground).bg(theme.background);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(" Commands ")
        .border_style(Style::default().fg(theme.accent))
        .style(base_style);
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let search = Paragraph::new(format!(" {}█", query))
        .block(Block::default().borders(Borders::ALL).title(" Search "))
        .style(base_style);
    f.render_widget(search, chunks[0]);

    let mut section = "";
    let mut items = Vec::new();
    let mut selected_row = None;
    for (offset, command) in visible.iter().enumerate() {
        if command.section != section {
            section = command.section;
            items.push(
                ListItem::new(Line::from(Span::styled(
                    format!(" {} ", section),
                    Style::default()
                        .fg(theme.selection_foreground)
                        .bg(theme.selection)
                        .add_modifier(Modifier::BOLD),
                )))
                .style(base_style),
            );
        }
        let key = app.config.key(command.action).unwrap_or_else(|| "-".into());
        let style = if start + offset == selected {
            selected_row = Some(items.len());
            Style::default()
                .fg(theme.selection_foreground)
                .bg(theme.selection)
                .add_modifier(Modifier::BOLD)
        } else {
            base_style
        };
        items.push(
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {key:<9}"),
                    Style::default()
                        .fg(theme.accent)
                        .bg(if start + offset == selected {
                            theme.selection
                        } else {
                            theme.background
                        }),
                ),
                Span::raw(command.description),
            ]))
            .style(style),
        );
    }
    if items.is_empty() {
        items.push(ListItem::new(" No matching commands").style(base_style));
    }
    let mut state = ListState::default();
    state.select(selected_row);
    f.render_stateful_widget(List::new(items).style(base_style), chunks[1], &mut state);
    f.render_widget(
        Paragraph::new(format!(
            " {}/{} matches | ↑/↓ select | Enter run | Esc close",
            commands.len(),
            COMMANDS.len()
        ))
        .style(Style::default().fg(theme.muted).bg(theme.background)),
        chunks[2],
    );
}

fn draw_input_modal(
    f: &mut TuiFrame,
    theme: Theme,
    title: &str,
    prompt: &str,
    input: &str,
    error: Option<&str>,
) {
    let area = centered_rect_cells(56, 8, f.area());
    let base_style = Style::default().fg(theme.foreground).bg(theme.background);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(theme.accent))
        .style(base_style);
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(inner);
    f.render_widget(Paragraph::new(prompt).style(base_style), chunks[0]);
    f.render_widget(
        Paragraph::new(format!(" {input}█"))
            .block(Block::default().borders(Borders::ALL))
            .style(base_style),
        chunks[1],
    );
    let footer = error.unwrap_or("Enter confirm | Esc cancel");
    let footer_color = if error.is_some() {
        TColor::Red
    } else {
        theme.muted
    };
    f.render_widget(
        Paragraph::new(footer).style(Style::default().fg(footer_color).bg(theme.background)),
        chunks[2],
    );
}

fn draw_animation_direction(f: &mut TuiFrame, app: &App, interval_ms: u64) {
    let key = |action| app.config.key(action).unwrap_or_else(|| "-".into());
    let text = format!(
        "Choose orbit direction\n\n{} left    {} down\n{} up      {} right\n\nDelay: {interval_ms} ms\nEsc cancel",
        key("orbit_left"),
        key("orbit_down"),
        key("orbit_up"),
        key("orbit_right"),
    );
    let area = centered_rect_cells(46, 10, f.area());
    let style = Style::default()
        .fg(app.theme.foreground)
        .bg(app.theme.background);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(" Animation Direction ")
        .border_style(Style::default().fg(app.theme.accent))
        .style(style);
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(text).block(block).style(style), area);
}

fn centered_rect_cells(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + r.width.saturating_sub(width) / 2;
    let y = r.y + r.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(r.width), height.min(r.height))
}

fn input_char(key: &str) -> Option<char> {
    if key == "space" {
        return Some(' ');
    }
    let mut chars = key.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

/// Expand a leading `~` to the user's home directory.
fn expand_home(input: &str) -> String {
    if input == "~" || input.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}{}", home.display(), &input[1..]);
        }
    }
    input.to_string()
}

/// Tab-complete the last path component against the filesystem. Returns the
/// (possibly extended) input and the current match names (directories get a
/// trailing `/`) so the modal can show them.
fn complete_path(input: &str) -> (String, Vec<String>) {
    let expanded = expand_home(input);
    let (dir_part, prefix) = match expanded.rfind('/') {
        Some(pos) => (&expanded[..=pos], &expanded[pos + 1..]),
        None => ("", expanded.as_str()),
    };
    let read_dir = if dir_part.is_empty() { "." } else { dir_part };
    let mut matches: Vec<String> = std::fs::read_dir(read_dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| {
                    let name = entry.file_name().into_string().ok()?;
                    let hidden = name.starts_with('.') && !prefix.starts_with('.');
                    if hidden || !name.starts_with(prefix) {
                        return None;
                    }
                    let suffix = if entry.path().is_dir() { "/" } else { "" };
                    Some(format!("{name}{suffix}"))
                })
                .collect()
        })
        .unwrap_or_default();
    matches.sort();
    if matches.is_empty() {
        return (expanded, matches);
    }
    // Extend the prefix to the longest common prefix of all matches.
    let mut lcp = matches[0].clone();
    for name in &matches[1..] {
        let keep = lcp
            .chars()
            .zip(name.chars())
            .take_while(|(a, b)| a == b)
            .count();
        let byte = lcp.char_indices().nth(keep).map_or(lcp.len(), |(i, _)| i);
        lcp.truncate(byte);
    }
    (format!("{dir_part}{lcp}"), matches)
}

fn draw_open_modal(f: &mut TuiFrame, theme: Theme, input: &str, error: Option<&str>) {
    let (_, matches) = complete_path(input);
    let shown: Vec<&str> = matches.iter().take(5).map(String::as_str).collect();
    let height = 8 + shown.len() as u16;
    let area = centered_rect_cells(64, height.min(f.area().height.saturating_sub(2)), f.area());
    let base_style = Style::default().fg(theme.foreground).bg(theme.background);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(" Add Meshes ")
        .border_style(Style::default().fg(theme.accent))
        .style(base_style);
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(shown.len() as u16),
            Constraint::Length(1),
        ])
        .split(inner);
    f.render_widget(
        Paragraph::new("Mesh file or directory (Tab completes, ~ expands)").style(base_style),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(format!(" {input}█"))
            .block(Block::default().borders(Borders::ALL))
            .style(base_style),
        chunks[1],
    );
    if !shown.is_empty() {
        let lines: Vec<Line> = shown.iter().map(|m| Line::from(format!(" {m}"))).collect();
        f.render_widget(
            Paragraph::new(lines).style(Style::default().fg(theme.muted).bg(theme.background)),
            chunks[2],
        );
    }
    let footer = error.unwrap_or("Enter load | Tab complete | Esc cancel");
    let footer_color = if error.is_some() {
        TColor::Red
    } else {
        theme.muted
    };
    f.render_widget(
        Paragraph::new(footer).style(Style::default().fg(footer_color).bg(theme.background)),
        chunks[3],
    );
}

/// Run the event-driven TUI.
pub fn run(mut app: App) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = event_loop(&mut app, &mut terminal);
    // Always clear the kitty image and restore the terminal.
    let mut out = io::stdout();
    let _ = write!(out, "{}", delete_image(IMAGE_ID));
    let _ = out.flush();
    ratatui::restore();
    result
}

fn event_loop(app: &mut App, terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut out = io::stdout();
    let theme_watcher = ThemeWatcher::new();
    let channel = image_channel();
    // Keep several unique file transfers alive so an asynchronous terminal
    // never observes a file being overwritten by the following frame.
    let mut file_images = VecDeque::new();
    loop {
        // Drain ALL pending events first (input coalescing): rapid key
        // repeats collapse into a single render of the latest state.
        while event::poll(Duration::from_millis(0))? {
            match event::read()? {
                Event::Key(ev)
                    if matches!(
                        ev.kind,
                        event::KeyEventKind::Press | event::KeyEventKind::Repeat
                    ) =>
                {
                    if let Some(key) = App::key_to_string(&ev) {
                        if app.handle_key(&key) {
                            return Ok(());
                        }
                    }
                }
                Event::Resize(_, _) => {
                    app.dirty = true;
                    app.render_dirty = true;
                }
                _ => {}
            }
        }

        // Live omarchy theme reload.
        if let Some(w) = &theme_watcher {
            if w.poll() {
                app.reload_theme();
            }
        }
        app.advance_animation(Instant::now());

        if app.dirty {
            terminal.draw(|f| app.draw_ui(f))?;

            // Hide the mesh canvas while a modal is up: some terminals
            // composite the low-z Kitty image over (or through) the modal's
            // cells, making text unreadable. Deleting the placement is the
            // only reliable occlusion; re-render the frame when it closes.
            if app.modal.is_some() {
                write!(out, "{}", delete_image(IMAGE_ID))?;
                out.flush()?;
                app.render_dirty = true; // restore the canvas after the modal
            } else if app.render_dirty {
                let area = terminal.size()?;
                let (vx, vy, vw, vh) = viewport_rect(app, area.width as u32, area.height as u32);
                if vw >= 4 && vh >= 2 {
                    let (px_w, px_h) =
                        pixel_size(app, area.width as u32, area.height as u32, vw, vh);
                    // One-shot: fit the initial framing to the real viewport
                    // aspect (no-op after the first frame, so resizes don't
                    // reset the user's zoom).
                    app.set_aspect(px_w as f32 / px_h as f32);
                    match app.render_frame(px_w, px_h) {
                        Some(frame) => {
                            transmit_frame(
                                &mut out,
                                &channel,
                                &mut file_images,
                                &frame,
                                vx,
                                vy,
                                vw,
                                vh,
                            )?;
                        }
                        None => {
                            write!(out, "{}", delete_image(IMAGE_ID))?;
                            out.flush()?;
                        }
                    }
                } else {
                    write!(out, "{}", delete_image(IMAGE_ID))?;
                    out.flush()?;
                }
                app.render_dirty = false;
            }
            app.dirty = false;
        }

        // Poll often enough for either live theme changes or animation.
        let wait = app.wait_duration(theme_watcher.is_some(), Instant::now());
        event::poll(wait)?;
    }
}

/// Push one frame to the terminal through the selected channel.
#[allow(clippy::too_many_arguments)]
fn transmit_frame(
    out: &mut impl Write,
    channel: &ImageChannel,
    file_images: &mut VecDeque<FileImage>,
    frame: &Frame,
    vx: u32,
    vy: u32,
    vw: u32,
    vh: u32,
) -> io::Result<()> {
    execute!(out, cursor::MoveTo(vx as u16, vy as u16))?;
    match channel {
        ImageChannel::File => {
            let mut img = FileImage::new().map_err(io::Error::other)?;
            img.write(&frame.pixels, frame.width, frame.height)
                .map_err(io::Error::other)?;
            write!(
                out,
                "{}",
                img.escape(IMAGE_ID, frame.width, frame.height, vw, vh)
            )?;
            file_images.push_back(img);
            if file_images.len() > 8 {
                file_images.pop_front();
            }
        }
        ImageChannel::SharedMemory => {
            let shm = meshtui_term::ShmImage::from_rgba(&frame.pixels, frame.width, frame.height)
                .map_err(io::Error::other)?;
            write!(
                out,
                "{}",
                shm.escape(IMAGE_ID, frame.width, frame.height, vw, vh)
            )?;
            out.flush()?;
            // Once flushed, the terminal owns and unlinks the POSIX shm name.
            shm.mark_transmitted();
            return Ok(());
        }
        ImageChannel::Direct => {
            for chunk in
                encode_png_fallback(&frame.pixels, frame.width, frame.height, IMAGE_ID, vw, vh)
                    .map_err(io::Error::other)?
            {
                out.write_all(chunk.as_bytes())?;
            }
        }
    }
    out.flush()
}

/// Recompute the viewport rect (must match `draw_ui`'s layout).
fn viewport_rect(app: &App, cols: u32, rows: u32) -> (u32, u32, u32, u32) {
    let sp = if app.show_sidepanel { 28 } else { 0 };
    let vw = cols.saturating_sub(sp);
    let vh = rows.saturating_sub(1); // status line
    (0, 0, vw, vh)
}

/// Render resolution for a viewport of `vw`×`vh` cells. Uses the real
/// cell pixel size from TIOCGWINSZ when the terminal reports it (Ghostty
/// and kitty do), falling back to the config estimate. Renders 1:1 with
/// display pixels (no visible downsampling), capped at MAX_RENDER_DIM.
fn pixel_size(app: &App, total_cols: u32, total_rows: u32, vw: u32, vh: u32) -> (u32, u32) {
    let (cell_w, cell_h) = match meshtui_term::terminal_pixel_size() {
        Some((pw, ph)) if total_cols > 0 && total_rows > 0 => (pw / total_cols, ph / total_rows),
        _ => (
            app.config.terminal.estimated_cell_width_px,
            app.config.terminal.estimated_cell_height_px,
        ),
    };
    let w = vw.saturating_mul(cell_w.max(1));
    let h = vh.saturating_mul(cell_h.max(1));
    let scale = (MAX_RENDER_DIM as f32 / w.max(h) as f32).min(1.0);
    (
        ((w as f32 * scale) as u32).max(1),
        ((h as f32 * scale) as u32).max(1),
    )
}

/// Save a freshly rendered frame as PNG (never a stale cached frame).
pub fn save_screenshot(
    app: &mut App,
    path: &std::path::Path,
    width: u32,
    height: u32,
) -> anyhow::Result<()> {
    let frame = app
        .render_frame(width, height)
        .ok_or_else(|| anyhow::anyhow!("nothing to screenshot: all meshes hidden"))?;
    // Always PNG: yazi's preview cache path has no extension, so the format
    // cannot be inferred from `path` and callers shouldn't depend on it.
    image::write_buffer_with_format(
        &mut std::io::BufWriter::new(std::fs::File::create(path)?),
        &frame.pixels,
        frame.width,
        frame.height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use meshtui_core::Mesh;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn triangle_at(name: &str, origin: Vec3, size: f32) -> Mesh {
        let mut mesh = Mesh::new(name);
        mesh.positions = vec![origin, origin + Vec3::X * size, origin + Vec3::Y * size];
        mesh.indices = vec![0, 1, 2];
        mesh.compute_normals();
        mesh
    }

    fn app_with_meshes(names: &[&str]) -> App {
        let mut scene = Scene::new();
        for name in names {
            scene.meshes.push(triangle_at(name, Vec3::ZERO, 1.0));
        }
        App::new(scene, Config::load(None).unwrap())
    }

    #[test]
    fn clear_filter_action_restores_all_meshes() {
        let mut app = app_with_meshes(&["beta", "alpha", "alpine"]);
        app.set_mesh_filter("alp").unwrap();
        assert_eq!(app.mesh_indices().len(), 2);
        app.execute_action("mesh_clear_filter");
        assert_eq!(app.mesh_indices().len(), 3, "clearing restores every mesh");
        assert!(app.mesh_filter.is_empty());
    }

    #[test]
    fn clear_filter_runs_from_the_command_palette() {
        let mut app = app_with_meshes(&["beta", "alpha", "alpine"]);
        app.set_mesh_filter("alp").unwrap();
        assert!(!app.handle_key("?"));
        for ch in "clear filter".chars() {
            let key = if ch == ' ' {
                "space".to_string()
            } else {
                ch.to_string()
            };
            assert!(!app.handle_key(&key));
        }
        assert!(!app.handle_key("enter"));
        assert!(app.modal.is_none(), "palette closed");
        assert_eq!(app.mesh_indices().len(), 3, "palette cleared the filter");
    }

    #[test]
    fn clear_filter_has_a_default_keybinding() {
        let app = app_with_meshes(&["mesh"]);
        let key = app.config.key("mesh_clear_filter");
        assert!(key.is_some(), "mesh_clear_filter must be bound by default");
        assert_eq!(
            app.action_for_key(&key.unwrap()).as_deref(),
            Some("mesh_clear_filter")
        );
    }

    #[test]
    fn selected_sidebar_row_keeps_the_mesh_color_dot() {
        let mut app = app_with_meshes(&["alpha"]);
        app.scene.meshes[0].color = [1.0, 0.0, 0.0, 1.0];
        let backend = TestBackend::new(100, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| app.draw_ui(frame)).unwrap();
        let buf = terminal.backend().buffer();
        let (dot_pos, dot) = buf
            .content()
            .iter()
            .enumerate()
            .find(|(_, cell)| cell.symbol() == "●")
            .expect("visible mesh dot");
        assert_eq!(
            dot.fg,
            TColor::Rgb(255, 0, 0),
            "selected row must keep the mesh color on the dot"
        );
        assert_eq!(
            dot.bg, app.theme.selection,
            "dot sits on the selection background"
        );
        // The mesh name on the same row still uses the selection colors.
        let y = dot_pos as u16 / buf.area.width;
        let name_cell = (0..buf.area.width)
            .map(|x| buf.cell((x, y)).unwrap())
            .find(|cell| cell.symbol() == "a")
            .expect("mesh name cell");
        assert_eq!(name_cell.fg, app.theme.selection_foreground);
    }

    #[test]
    fn delete_removes_selected_mesh_and_undo_restores_it() {
        let mut app = app_with_meshes(&["beta", "alpha", "alpine"]);
        assert_eq!(app.selected, 1, "alphabetical first is alpha");
        app.execute_action("mesh_delete");
        assert_eq!(app.scene.meshes.len(), 2);
        assert!(!app.scene.meshes.iter().any(|mesh| mesh.name == "alpha"));
        // Selection moved to the first remaining (alphabetical) mesh.
        assert_eq!(app.scene.meshes[app.selected].name, "alpine");

        app.execute_action("mesh_undo_delete");
        assert_eq!(app.scene.meshes.len(), 3);
        assert_eq!(
            app.scene.meshes[app.selected].name, "alpha",
            "undo selects the first restored mesh"
        );
    }

    #[test]
    fn delete_remaps_marks_and_undo_restores_them() {
        let mut app = app_with_meshes(&["beta", "alpha", "alpine", "zeta"]);
        // Mark beta (0) and alpine (2): delete removes the marked meshes.
        app.marked = [0, 2].into_iter().collect();
        app.execute_action("mesh_delete");
        assert_eq!(
            app.scene
                .meshes
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert!(app.marked.is_empty(), "deleted marks leave the active set");

        app.execute_action("mesh_undo_delete");
        assert_eq!(
            app.scene
                .meshes
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["beta", "alpha", "alpine", "zeta"],
            "undo reinserts meshes at their original positions"
        );
        assert_eq!(app.marked, [0, 2].into_iter().collect());
    }

    #[test]
    fn delete_shifts_marks_outside_the_delete_scope() {
        let mut app = app_with_meshes(&["beta", "alpha", "alpine", "zeta"]);
        app.set_mesh_filter("alp").unwrap(); // in scope: alpha (1), alpine (2)
        app.marked = [3].into_iter().collect(); // zeta, outside the filter scope
        app.execute_action("mesh_delete"); // deletes the selected alpha (1)
        assert_eq!(
            app.scene
                .meshes
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["beta", "alpine", "zeta"]
        );
        assert_eq!(
            app.marked,
            [2].into_iter().collect(),
            "zeta's mark shifts down past the removed index"
        );
        assert_eq!(app.scene.meshes[app.selected].name, "alpine");

        app.execute_action("mesh_undo_delete");
        assert_eq!(
            app.scene
                .meshes
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["beta", "alpha", "alpine", "zeta"]
        );
        assert_eq!(app.marked, [3].into_iter().collect());
    }

    #[test]
    fn delete_respects_the_active_filter() {
        let mut app = app_with_meshes(&["beta", "alpha", "alpine"]);
        app.set_mesh_filter("alp").unwrap();
        app.execute_action("mesh_select_all");
        app.execute_action("mesh_delete");
        assert_eq!(app.scene.meshes.len(), 1);
        assert_eq!(
            app.scene.meshes[0].name, "beta",
            "filtered-out beta survives"
        );
        assert!(
            app.mesh_indices().is_empty(),
            "the surviving beta does not match the active filter"
        );
        app.execute_action("mesh_clear_filter");
        assert_eq!(app.mesh_indices(), &[0]);
    }

    #[test]
    fn deleting_everything_leaves_a_usable_empty_scene() {
        let mut app = app_with_meshes(&["mesh"]);
        app.execute_action("mesh_delete");
        assert!(app.scene.meshes.is_empty());
        assert!(app.mesh_indices().is_empty());
        assert!(
            app.render_frame(64, 32).is_none(),
            "empty scene renders None"
        );
        app.execute_action("mesh_undo_delete");
        assert_eq!(app.scene.meshes.len(), 1);
    }

    #[test]
    fn add_meshes_appends_selects_and_respects_filter() {
        let mut app = app_with_meshes(&["beta"]);
        let added = app.add_meshes(vec![
            triangle_at("alpha", Vec3::ZERO, 1.0),
            triangle_at("gamma", Vec3::ZERO, 1.0),
        ]);
        assert_eq!(added, 2);
        assert_eq!(app.scene.meshes.len(), 3);
        assert_eq!(
            app.scene.meshes[app.selected].name, "alpha",
            "selection jumps to the first added mesh"
        );

        // With a filter active, added meshes appear only when they match.
        let mut app = app_with_meshes(&["beta"]);
        app.set_mesh_filter("zzz").unwrap();
        assert!(app.mesh_indices().is_empty());
        app.add_meshes(vec![triangle_at("zzz_mesh", Vec3::ZERO, 1.0)]);
        assert_eq!(app.mesh_indices().len(), 1);
        assert_eq!(app.scene.meshes[app.selected].name, "zzz_mesh");
    }

    #[test]
    fn path_completion_expands_and_matches() {
        let dir = std::env::temp_dir().join(format!("meshtui_test_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("subdir")).unwrap();
        std::fs::write(dir.join("alpha.ply"), b"").unwrap();
        std::fs::write(dir.join("alpine.obj"), b"").unwrap();
        let base = dir.display().to_string();

        // Tilde expansion.
        let home = dirs::home_dir().unwrap();
        let (expanded, _) = complete_path("~/");
        assert_eq!(expanded, format!("{}/", home.display()));

        // Unique directory component completes with a trailing slash.
        let (completed, matches) = complete_path(&format!("{base}/sub"));
        assert_eq!(completed, format!("{base}/subdir/"));
        assert_eq!(matches, vec!["subdir/".to_string()]);

        // Ambiguous prefix completes to the longest common prefix.
        let (completed, matches) = complete_path(&format!("{base}/al"));
        assert_eq!(completed, format!("{base}/alp"));
        assert_eq!(matches.len(), 2);
        let (completed, _) = complete_path(&format!("{base}/alpi"));
        assert_eq!(completed, format!("{base}/alpine.obj"));

        // Missing directory: input unchanged, no matches.
        let (completed, matches) = complete_path(&format!("{base}/nope/x"));
        assert_eq!(completed, format!("{base}/nope/x"));
        assert!(matches.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_modal_loads_meshes_from_disk() {
        let testdata = concat!(env!("CARGO_MANIFEST_DIR"), "/../meshtui-core/testdata");
        if !std::path::Path::new(testdata).is_dir() {
            return; // testdata not generated in this checkout
        }
        let mut app = app_with_meshes(&["existing"]);
        app.modal = Some(Modal::OpenPath {
            input: testdata.to_string(),
            error: None,
        });
        assert!(!app.handle_key("enter"));
        assert!(app.modal.is_none(), "modal closed after successful load");
        assert!(app.scene.meshes.len() > 1, "directory meshes were added");
    }

    #[test]
    fn up_vector_cycle_reorients_and_reverses() {
        // Default config has [Y-up, Z-up]; u/U must walk between them.
        let mut app = app_with_meshes(&["mesh"]);
        let up0 = app.camera.up();
        app.execute_action("up_vector_next");
        let up1 = app.camera.up();
        assert_ne!(up0, up1, "u should change the up vector");
        app.execute_action("up_vector_prev");
        let up2 = app.camera.up();
        assert!(
            up0.dot(up2) > 0.999,
            "U should return to the original up: {up0:?} vs {up2:?}"
        );
    }

    #[test]
    fn palette_runs_selected_command() {
        let mut app = app_with_meshes(&["mesh"]);
        app.modal = Some(Modal::CommandPalette {
            query: "perspective camera".into(),
            selected: 0,
        });
        assert!(!app.handle_key("enter"));
        assert_eq!(app.camera.kind, CameraKind::Perspective);
        assert!(app.modal.is_none());
    }

    #[test]
    fn switching_camera_kind_refits_distance_but_keeps_orientation() {
        let mut app = app_with_meshes(&["mesh"]);
        // Give the scene real extent and an orbit so we can verify they survive.
        app.scene.meshes[0] = triangle_at("mesh", Vec3::ZERO, 4.0);
        app.camera.orbit(0.4, -0.2);
        let orientation = app.camera.orientation;
        app.set_aspect(2.0);
        app.frame_visible_meshes();

        let ortho_distance = app.camera.distance;
        app.execute_action("camera_perspective");
        assert_eq!(app.camera.kind, CameraKind::Perspective);
        let persp_distance = app.camera.distance;
        assert_ne!(
            ortho_distance, persp_distance,
            "distance must be re-fit for the new projection"
        );
        assert_eq!(
            app.camera.orientation, orientation,
            "orbit must be preserved across the switch"
        );

        // Switching back to the same kind is a no-op for distance.
        app.execute_action("camera_orthographic");
        let back = app.camera.distance;
        app.execute_action("camera_orthographic");
        assert_eq!(app.camera.distance, back);
        assert_eq!(app.camera.orientation, orientation);
    }

    #[test]
    fn hiding_a_mesh_recenters_and_refits_the_remaining_mesh() {
        let mut scene = Scene::new();
        scene.meshes.push(triangle_at("near", Vec3::ZERO, 1.0));
        scene
            .meshes
            .push(triangle_at("far", Vec3::new(100.0, 0.0, 0.0), 1.0));
        let mut app = App::new(scene, Config::load(None).unwrap());
        app.camera.orbit(0.3, -0.1);
        let orientation = app.camera.orientation;
        let combined_distance = app.camera.distance;
        let combined_target = app.camera.target;

        assert_eq!(app.selected, 1, "alphabetical first mesh is far");
        app.execute_action("mesh_hide");

        let remaining = app.scene.meshes[0].bounds().unwrap();
        let expected_target = (remaining.0 + remaining.1) * 0.5;
        assert!(
            (app.camera.target - expected_target).length() < 1e-4,
            "target={:?} expected={expected_target:?}",
            app.camera.target
        );
        assert!(
            (combined_target - app.camera.target).length() > 10.0,
            "camera should leave the combined-scene center"
        );
        assert!(
            app.camera.distance < combined_distance * 0.5,
            "distance={:.3} combined={:.3}",
            app.camera.distance,
            combined_distance
        );
        assert_eq!(app.camera.orientation, orientation);
    }

    #[test]
    fn auto_zoom_off_keeps_camera_fixed_when_meshes_change() {
        let mut scene = Scene::new();
        scene.meshes.push(triangle_at("near", Vec3::ZERO, 1.0));
        scene
            .meshes
            .push(triangle_at("far", Vec3::new(100.0, 0.0, 0.0), 1.0));
        let mut app = App::new(scene, Config::load(None).unwrap());
        let distance = app.camera.distance;
        let target = app.camera.target;

        // Toggle off, then hide the far mesh: framing must not move.
        app.execute_action("toggle_auto_zoom");
        assert_eq!(
            app.status_message.as_deref(),
            Some("auto-zoom off (camera distance fixed)")
        );
        assert_eq!(app.selected, 1, "alphabetical first mesh is far");
        app.execute_action("mesh_hide");
        assert!(!app.scene.meshes[1].visible);
        assert_eq!(app.camera.distance, distance, "no refit with auto-zoom off");
        assert_eq!(app.camera.target, target, "no recenter with auto-zoom off");

        // A deliberate projection switch still refits (it must, or the scene
        // would clip), even with auto-zoom off.
        app.execute_action("camera_perspective");
        assert_ne!(app.camera.distance, distance);

        // Toggle back on: the next visibility change reframes again.
        app.execute_action("toggle_auto_zoom");
        app.execute_action("mesh_show");
        assert!(
            (app.camera.target.x - 50.0).abs() < 1.0,
            "re-enabled auto-zoom reframes the combined scene, target={:?}",
            app.camera.target
        );
    }

    #[test]
    fn hiding_every_mesh_leaves_the_camera_unchanged() {
        let mut app = app_with_meshes(&["mesh"]);
        let target = app.camera.target;
        let distance = app.camera.distance;
        app.execute_action("mesh_hide");
        assert!(!app.scene.meshes[0].visible);
        assert_eq!(app.camera.target, target);
        assert_eq!(app.camera.distance, distance);
    }

    #[test]
    fn showing_a_mesh_reframes_to_visible_bounds() {
        let mut scene = Scene::new();
        scene.meshes.push(triangle_at("near", Vec3::ZERO, 1.0));
        scene
            .meshes
            .push(triangle_at("far", Vec3::new(100.0, 0.0, 0.0), 1.0));
        let mut app = App::new(scene, Config::load(None).unwrap());
        assert_eq!(app.selected, 1, "alphabetical first mesh is far");
        app.execute_action("mesh_hide");
        let near_only_target = app.camera.target;

        app.execute_action("mesh_show");
        assert!(app.scene.meshes[1].visible);
        assert!(
            (app.camera.target - near_only_target).length() > 10.0,
            "showing the far mesh should pull the camera back toward the combined center"
        );
    }

    #[test]
    fn marked_mesh_actions_are_bulk_actions() {
        let mut app = app_with_meshes(&["beta", "alpha"]);
        assert_eq!(app.selected, 1, "alphabetical first mesh is selected");
        app.execute_action("mesh_toggle_mark");
        app.execute_action("sidepanel_move_down");
        app.execute_action("mesh_toggle_mark");
        app.execute_action("mesh_hide");
        assert!(app.scene.meshes.iter().all(|mesh| !mesh.visible));
    }

    #[test]
    fn mesh_filter_limits_selection() {
        let mut app = app_with_meshes(&["beta", "alpha", "alpine"]);
        app.set_mesh_filter("alp").unwrap();
        let names: Vec<&str> = app
            .mesh_indices()
            .iter()
            .map(|&index| app.scene.meshes[index].name.as_str())
            .collect();
        assert_eq!(names, ["alpha", "alpine"]);
    }

    #[test]
    fn regex_filter_is_case_insensitive_and_reports_errors() {
        let mut app = app_with_meshes(&["beta.stl", "ALPHA.ply", "alpine.obj"]);
        app.set_mesh_filter(r"re:^alp.*\.(ply|obj)$").unwrap();
        let names: Vec<&str> = app
            .mesh_indices()
            .iter()
            .map(|&index| app.scene.meshes[index].name.as_str())
            .collect();
        assert_eq!(names, ["ALPHA.ply", "alpine.obj"]);

        assert!(app.set_mesh_filter("re:[").is_err());
        assert_eq!(
            app.mesh_filter, r"re:^alp.*\.(ply|obj)$",
            "an invalid regex must preserve the active filter"
        );
    }

    #[test]
    fn inverted_filter_excludes_matching_names() {
        let mut app = app_with_meshes(&["molar", "incisor_tooth", "tooth_42", "jaw"]);
        app.set_mesh_filter("!tooth").unwrap();
        let names: Vec<&str> = app
            .mesh_indices()
            .iter()
            .map(|&index| app.scene.meshes[index].name.as_str())
            .collect();
        assert_eq!(names, ["jaw", "molar"]);
    }

    #[test]
    fn inverted_regex_filter_excludes_matches() {
        let mut app = app_with_meshes(&["beta", "alpha", "alpine"]);
        app.set_mesh_filter("!re:alp").unwrap();
        let names: Vec<&str> = app
            .mesh_indices()
            .iter()
            .map(|&index| app.scene.meshes[index].name.as_str())
            .collect();
        assert_eq!(names, ["beta"]);
    }

    #[test]
    fn bulk_color_cycle_applies_one_shared_color() {
        let mut app = app_with_meshes(&["beta", "alpha"]);
        let palette = app.config.ui.sidepanel.color_palette.clone();
        app.scene.meshes[0].color = palette[0];
        app.scene.meshes[1].color = palette[3];
        app.execute_action("mesh_select_all");
        app.execute_action("mesh_color_next");
        // Targets are alphabetical, so "alpha" (index 1) is the cycle source.
        assert_eq!(app.scene.meshes[0].color, palette[4]);
        assert_eq!(app.scene.meshes[1].color, palette[4]);
        app.execute_action("mesh_color_prev");
        assert_eq!(app.scene.meshes[0].color, palette[3]);
        assert_eq!(app.scene.meshes[1].color, palette[3]);
    }

    #[test]
    fn select_all_targets_only_filtered_meshes() {
        let mut app = app_with_meshes(&["beta", "alpha", "alpine"]);
        app.set_mesh_filter("alp").unwrap();
        app.execute_action("mesh_select_all");
        assert_eq!(app.marked.len(), 2);
        app.execute_action("mesh_hide");

        assert!(
            app.scene.meshes[0].visible,
            "non-matching beta stays visible"
        );
        assert!(!app.scene.meshes[1].visible);
        assert!(!app.scene.meshes[2].visible);

        app.execute_action("mesh_select_none");
        assert!(app.marked.is_empty());
    }

    #[test]
    fn hidden_marks_do_not_escape_filter_scope() {
        let mut app = app_with_meshes(&["beta", "alpha"]);
        app.execute_action("mesh_select_all");
        app.set_mesh_filter("alpha").unwrap();
        app.execute_action("mesh_hide");
        assert!(
            app.scene.meshes[0].visible,
            "filtered-out beta stays visible"
        );
        assert!(!app.scene.meshes[1].visible);
    }

    #[test]
    fn every_modal_cell_has_an_opaque_background() {
        let modals = [
            Modal::CommandPalette {
                query: String::new(),
                selected: 0,
            },
            Modal::HexColor {
                input: String::new(),
                error: None,
            },
            Modal::AnimationDelay {
                input: String::new(),
                error: None,
            },
            Modal::AnimationDirection { interval_ms: 100 },
            Modal::MeshFilter {
                input: String::new(),
                error: None,
            },
            Modal::OpenPath {
                input: String::new(),
                error: None,
            },
        ];
        for modal in modals {
            let mut app = app_with_meshes(&["mesh"]);
            app.modal = Some(modal);
            let backend = TestBackend::new(100, 40);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|frame| app.draw_ui(frame)).unwrap();
            let cell = terminal.backend().buffer().cell((50, 20)).unwrap();
            assert_ne!(cell.bg, TColor::Reset);
        }
    }

    #[test]
    fn command_palette_area_is_fully_opaque() {
        let mut app = app_with_meshes(&["mesh"]);
        app.modal = Some(Modal::CommandPalette {
            query: String::new(),
            selected: 0,
        });
        let backend = TestBackend::new(100, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| app.draw_ui(frame)).unwrap();
        let buf = terminal.backend().buffer();
        // Recompute the palette rect as draw_command_palette does, then assert
        // every cell in it is opaque (so nothing bleeds through).
        let commands = app.palette_matches("");
        let max_items = app.config.ui.command_palette.max_visible_items;
        let visible = &commands[..commands.len().min(max_items)];
        let section_count = visible
            .iter()
            .map(|c| c.section)
            .fold(Vec::<&str>::new(), |mut s, sec| {
                if s.last().copied() != Some(sec) {
                    s.push(sec);
                }
                s
            })
            .len();
        let height = (visible.len() + section_count + 6) as u16;
        let area = centered_rect_cells(70, height.min(buf.area.height.saturating_sub(2)), buf.area);
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                let cell = buf.cell((x, y)).unwrap();
                assert_ne!(
                    cell.bg,
                    TColor::Reset,
                    "transparent cell at ({x},{y}) in command palette"
                );
            }
        }
    }

    #[test]
    fn animation_advances_camera() {
        let mut app = app_with_meshes(&["mesh"]);
        let orientation = app.camera.orientation;
        app.start_animation("orbit_left", 1);
        app.advance_animation(Instant::now() + Duration::from_millis(2));
        assert_ne!(app.camera.orientation, orientation);
        assert!(app.render_dirty);
    }

    #[test]
    fn animation_can_defer_rendering_until_stopped() {
        let mut app = app_with_meshes(&["mesh"]);
        app.config.ui.animation.render_on_animate = false;
        app.render_dirty = false;
        app.start_animation("orbit_right", 1);
        app.advance_animation(Instant::now() + Duration::from_millis(2));
        assert!(!app.render_dirty);
        app.execute_action("animation_stop");
        assert!(app.render_dirty);
    }

    #[test]
    fn animation_catches_up_motion_but_caps_render_rate() {
        let mut app = app_with_meshes(&["mesh"]);
        app.config.ui.animation.animation_fps = 10;
        app.render_dirty = false;
        app.start_animation("orbit_right", 50);
        let first_step = app.animation.as_ref().unwrap().next_step;
        let mut expected = app.camera.clone();
        expected.orbit(3.0 * app.config.orbital_camera.movement_speed, 0.0);
        app.advance_animation(first_step + Duration::from_millis(125));
        assert!(
            (app.camera.position() - expected.position()).length() < 1e-4,
            "pos={:?} expected={:?}",
            app.camera.position(),
            expected.position()
        );
        assert!(app.render_dirty);

        app.render_dirty = false;
        app.advance_animation(first_step + Duration::from_millis(150));
        assert!(
            !app.render_dirty,
            "second frame must respect the 10 FPS cap"
        );
    }
}
