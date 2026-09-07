use meshtui_core::Color;

#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    pub section: &'static str,
    pub action: &'static str,
    pub description: &'static str,
}

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        section: "Camera",
        action: "view_minus_x",
        description: "View from -X axis",
    },
    CommandSpec {
        section: "Camera",
        action: "view_plus_x",
        description: "View from +X axis",
    },
    CommandSpec {
        section: "Camera",
        action: "view_minus_y",
        description: "View from -Y axis",
    },
    CommandSpec {
        section: "Camera",
        action: "view_plus_y",
        description: "View from +Y axis",
    },
    CommandSpec {
        section: "Camera",
        action: "view_minus_z",
        description: "View from -Z axis",
    },
    CommandSpec {
        section: "Camera",
        action: "view_plus_z",
        description: "View from +Z axis",
    },
    CommandSpec {
        section: "Camera",
        action: "orbit_left",
        description: "Orbit left",
    },
    CommandSpec {
        section: "Camera",
        action: "orbit_right",
        description: "Orbit right",
    },
    CommandSpec {
        section: "Camera",
        action: "orbit_up",
        description: "Orbit up",
    },
    CommandSpec {
        section: "Camera",
        action: "orbit_down",
        description: "Orbit down",
    },
    CommandSpec {
        section: "Camera",
        action: "orbit_left_fast",
        description: "Orbit left (fast)",
    },
    CommandSpec {
        section: "Camera",
        action: "orbit_right_fast",
        description: "Orbit right (fast)",
    },
    CommandSpec {
        section: "Camera",
        action: "orbit_up_fast",
        description: "Orbit up (fast)",
    },
    CommandSpec {
        section: "Camera",
        action: "orbit_down_fast",
        description: "Orbit down (fast)",
    },
    CommandSpec {
        section: "Camera",
        action: "zoom_in",
        description: "Zoom in",
    },
    CommandSpec {
        section: "Camera",
        action: "zoom_out",
        description: "Zoom out",
    },
    CommandSpec {
        section: "Camera",
        action: "reset_orbital",
        description: "Reset camera",
    },
    CommandSpec {
        section: "Camera",
        action: "camera_orthographic",
        description: "Use orthographic camera",
    },
    CommandSpec {
        section: "Camera",
        action: "camera_perspective",
        description: "Use perspective camera",
    },
    CommandSpec {
        section: "Camera",
        action: "up_vector_next",
        description: "Use next up vector",
    },
    CommandSpec {
        section: "Camera",
        action: "up_vector_prev",
        description: "Use previous up vector",
    },
    CommandSpec {
        section: "Rendering",
        action: "light_decrease",
        description: "Decrease light intensity",
    },
    CommandSpec {
        section: "Rendering",
        action: "light_increase",
        description: "Increase light intensity",
    },
    CommandSpec {
        section: "Rendering",
        action: "wireframe_increase",
        description: "Enable or thicken wireframe",
    },
    CommandSpec {
        section: "Rendering",
        action: "wireframe_off",
        description: "Disable wireframe",
    },
    CommandSpec {
        section: "Animation",
        action: "animation_start",
        description: "Configure and start orbit animation",
    },
    CommandSpec {
        section: "Animation",
        action: "animation_stop",
        description: "Stop orbit animation",
    },
    CommandSpec {
        section: "Meshes",
        action: "sidepanel_move_up",
        description: "Select previous mesh",
    },
    CommandSpec {
        section: "Meshes",
        action: "sidepanel_move_down",
        description: "Select next mesh",
    },
    CommandSpec {
        section: "Meshes",
        action: "mesh_toggle_mark",
        description: "Mark or unmark selected mesh",
    },
    CommandSpec {
        section: "Meshes",
        action: "mesh_select_all",
        description: "Select all meshes matching the filter",
    },
    CommandSpec {
        section: "Meshes",
        action: "mesh_select_none",
        description: "Clear the mesh selection",
    },
    CommandSpec {
        section: "Meshes",
        action: "mesh_filter",
        description: "Filter meshes by substring or regex (! inverts)",
    },
    CommandSpec {
        section: "Meshes",
        action: "mesh_clear_filter",
        description: "Clear mesh-name filter",
    },
    CommandSpec {
        section: "Meshes",
        action: "mesh_hide",
        description: "Hide selected or marked meshes",
    },
    CommandSpec {
        section: "Meshes",
        action: "mesh_show",
        description: "Show selected or marked meshes",
    },
    CommandSpec {
        section: "Meshes",
        action: "mesh_color_next",
        description: "Cycle mesh color forward",
    },
    CommandSpec {
        section: "Meshes",
        action: "mesh_color_prev",
        description: "Cycle mesh color backward",
    },
    CommandSpec {
        section: "Meshes",
        action: "mesh_custom_color",
        description: "Set custom mesh color",
    },
    CommandSpec {
        section: "Meshes",
        action: "reset_single_mesh",
        description: "Reset selected or marked meshes",
    },
    CommandSpec {
        section: "Meshes",
        action: "reset_all_meshes",
        description: "Reset all meshes",
    },
    CommandSpec {
        section: "Interface",
        action: "toggle_sidepanel",
        description: "Toggle side panel",
    },
    CommandSpec {
        section: "Interface",
        action: "toggle_scene_info",
        description: "Toggle scene information",
    },
    CommandSpec {
        section: "Interface",
        action: "screenshot",
        description: "Save screenshot",
    },
    CommandSpec {
        section: "Interface",
        action: "quit",
        description: "Quit",
    },
];

pub fn fuzzy_score(query: &str, candidate: &str) -> f32 {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return 1.0;
    }
    let candidate = candidate.to_lowercase();
    if candidate.contains(&query) {
        return 1.0;
    }

    let mut matched = 0usize;
    let mut consecutive = 0usize;
    let mut best_run = 0usize;
    let mut query_chars = query.chars();
    let mut wanted = query_chars.next();
    for ch in candidate.chars() {
        if Some(ch) == wanted {
            matched += 1;
            consecutive += 1;
            best_run = best_run.max(consecutive);
            wanted = query_chars.next();
            if wanted.is_none() {
                break;
            }
        } else {
            consecutive = 0;
        }
    }
    let query_len = query.chars().count();
    if matched != query_len || query_len == 0 {
        return 0.0;
    }
    let candidate_len = candidate.chars().count().max(1);
    0.7 * query_len as f32 / candidate_len as f32 + 0.3 * best_run as f32 / query_len as f32
}

pub fn parse_hex_color(input: &str) -> Result<Color, &'static str> {
    let hex = input.trim().strip_prefix('#').unwrap_or(input.trim());
    if hex.len() != 6 {
        return Err("Use six hex digits: #RRGGBB");
    }
    let value = u32::from_str_radix(hex, 16).map_err(|_| "Invalid hex color")?;
    Ok([
        ((value >> 16) & 0xff) as f32 / 255.0,
        ((value >> 8) & 0xff) as f32 / 255.0,
        (value & 0xff) as f32 / 255.0,
        1.0,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_score_matches_subsequence() {
        assert!(fuzzy_score("cam", "Use perspective camera") > 0.2);
        assert_eq!(fuzzy_score("xyz", "Orbit left"), 0.0);
    }

    #[test]
    fn parses_hex_with_optional_hash() {
        assert_eq!(
            parse_hex_color("#ff8000").unwrap(),
            [1.0, 128.0 / 255.0, 0.0, 1.0]
        );
        assert!(parse_hex_color("bad").is_err());
    }
}
