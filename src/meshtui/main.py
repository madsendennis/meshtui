"""Main entry point for meshtui CLI."""

import math
import signal
import sys
import termios
import tty
from collections import defaultdict
from pathlib import Path
from typing import Any, TypedDict

import trimesh

from meshtui import config
from meshtui.kitty_protocol import (
    clear_images,
    display_image,
    get_terminal_bg_ansi,
    get_terminal_size,
)
from meshtui.mesh_loader import load_mesh
from meshtui.renderer import render_mesh

# Load configuration defaults
_CAMERA_CONFIG = config.get_camera_config()
_VIEW_CONFIG = config.get_view_config()
_WIREFRAME_CONFIG = config.get_wireframe_config()
_ORBITAL_CONFIG = config.get_orbital_camera_config()
_KEYBINDINGS_CONFIG = config.get_keybindings_config()

# Special key mappings for raw terminal input
_SPECIAL_KEYS = {
    "esc": "\x1b",
    "space": " ",
    "tab": "\t",
    "enter": "\r",
    "backspace": "\x7f",
    # Add more if needed, like ctrl combinations
}


class KeyDispatcher:
    """Handles keybinding dispatch for configurable input."""

    def __init__(self, keybindings: dict[str, Any]):
        self.key_to_action: dict[str, str] = {}
        self._action_to_display_keys: dict[str, list[str]] = defaultdict(list)
        self._build_key_map(keybindings)

    def _build_key_map(self, keybindings: dict[str, Any]) -> None:
        """Build the key to action mapping from config."""
        for action, keys in keybindings.items():
            if isinstance(keys, str):
                keys = [keys]
            elif not isinstance(keys, list):
                continue  # Skip invalid entries

            for key in keys:
                key_code = _SPECIAL_KEYS.get(key, key)
                self.key_to_action[key_code] = action

            # Also build display keys
            for key in keys if isinstance(keys, list) else [keys]:
                self._action_to_display_keys[action].append(key)

    def get_action(self, key: str) -> str | None:
        """Get the action for a given key press."""
        return self.key_to_action.get(key)


# Global state for handling terminal resize
_current_mesh: trimesh.Trimesh | None = None
_resize_pending = False
_view_axis = _VIEW_CONFIG["default_axis"]
_wireframe_thickness = _WIREFRAME_CONFIG["default_thickness"]
_camera_type = _CAMERA_CONFIG["type"]
_show_help = False
_up_vector_cycle_index = -1  # -1 means use default (index 0)
_up_vector_override: tuple[float, float, float] | None = None
_last_render_params: dict[str, Any] = {}
_last_image_data: bytes | None = None


# Load up vector list from config (index 0 is the default)
_UP_VECTORS: list[tuple[float, float, float]] = [tuple(v) for v in _VIEW_CONFIG["up_vectors"]]


# Action functions for key dispatcher
def action_quit() -> bool:
    """Quit the application."""
    sys.exit(0)


def action_view_minus_x() -> bool:
    global _view_axis, _orbital_active, _up_vector_override, _up_vector_cycle_index
    _view_axis = "-x"
    _orbital_active = False
    _up_vector_override = _get_default_up_vector_for_axis("-x")
    _up_vector_cycle_index = -1
    return True


def action_view_plus_x() -> bool:
    global _view_axis, _orbital_active, _up_vector_override, _up_vector_cycle_index
    _view_axis = "+x"
    _orbital_active = False
    _up_vector_override = _get_default_up_vector_for_axis("+x")
    _up_vector_cycle_index = -1
    return True


def action_view_minus_y() -> bool:
    global _view_axis, _orbital_active, _up_vector_override, _up_vector_cycle_index
    _view_axis = "-y"
    _orbital_active = False
    _up_vector_override = _get_default_up_vector_for_axis("-y")
    _up_vector_cycle_index = -1
    return True


def action_view_plus_y() -> bool:
    global _view_axis, _orbital_active, _up_vector_override, _up_vector_cycle_index
    _view_axis = "+y"
    _orbital_active = False
    _up_vector_override = _get_default_up_vector_for_axis("+y")
    _up_vector_cycle_index = -1
    return True


def action_view_minus_z() -> bool:
    global _view_axis, _orbital_active, _up_vector_override, _up_vector_cycle_index
    _view_axis = "-z"
    _orbital_active = False
    _up_vector_override = _get_default_up_vector_for_axis("-z")
    _up_vector_cycle_index = -1
    return True


def action_view_plus_z() -> bool:
    global _view_axis, _orbital_active, _up_vector_override, _up_vector_cycle_index
    _view_axis = "+z"
    _orbital_active = False
    _up_vector_override = _get_default_up_vector_for_axis("+z")
    _up_vector_cycle_index = -1
    return True


def action_wireframe_off() -> bool:
    global _wireframe_thickness
    _wireframe_thickness = 0.0
    return True


def action_wireframe_increase() -> bool:
    global _wireframe_thickness
    if _wireframe_thickness == 0.0:
        _wireframe_thickness = 1.0
    else:
        _wireframe_thickness += 1.0
    return True


def action_toggle_camera_type() -> bool:
    global _camera_type
    _camera_type = "orthographic" if _camera_type == "perspective" else "perspective"
    return True


def action_light_decrease() -> bool:
    global _light_intensity
    _light_intensity = max(0.1, _light_intensity - 0.5)
    return True


def action_light_increase() -> bool:
    global _light_intensity
    _light_intensity = min(10.0, _light_intensity + 0.5)
    return True


def action_up_vector_next() -> bool:
    global _up_vector_cycle_index, _up_vector_override
    if _up_vector_cycle_index == -1:
        _up_vector_cycle_index = 0
    else:
        _up_vector_cycle_index = (_up_vector_cycle_index + 1) % len(_UP_VECTORS)
    _up_vector_override = _UP_VECTORS[_up_vector_cycle_index]
    return True


def action_up_vector_prev() -> bool:
    global _up_vector_cycle_index, _up_vector_override
    if _up_vector_cycle_index == -1:
        _up_vector_cycle_index = len(_UP_VECTORS) - 1
    else:
        _up_vector_cycle_index = (_up_vector_cycle_index - 1) % len(_UP_VECTORS)
    _up_vector_override = _UP_VECTORS[_up_vector_cycle_index]
    return True


def action_help_toggle() -> bool:
    global _show_help
    _show_help = not _show_help
    return True


def action_help_close() -> bool | str:
    global _show_help
    if _show_help:
        _show_help = False
        return "force_redraw"
    return False


def action_orbit_left() -> bool:
    global _orbital_active, _orbital_theta
    if not _orbital_active:
        if _current_mesh is not None:
            _initialize_orbital_camera(_current_mesh)
        _initialize_orbital_from_view_axis(_view_axis)
    _orbital_active = True
    _orbital_theta -= _ORBITAL_CONFIG["movement_speed"]
    return True


def action_orbit_right() -> bool:
    global _orbital_active, _orbital_theta
    if not _orbital_active:
        if _current_mesh is not None:
            _initialize_orbital_camera(_current_mesh)
        _initialize_orbital_from_view_axis(_view_axis)
    _orbital_active = True
    _orbital_theta += _ORBITAL_CONFIG["movement_speed"]
    return True


def action_orbit_down() -> bool:
    global _orbital_active, _orbital_phi
    if not _orbital_active:
        if _current_mesh is not None:
            _initialize_orbital_camera(_current_mesh)
        _initialize_orbital_from_view_axis(_view_axis)
    _orbital_active = True
    _orbital_phi += _ORBITAL_CONFIG["movement_speed"]
    _orbital_phi = max(0.1, min(math.pi - 0.1, _orbital_phi))
    return True


def action_orbit_up() -> bool:
    global _orbital_active, _orbital_phi
    if not _orbital_active:
        if _current_mesh is not None:
            _initialize_orbital_camera(_current_mesh)
        _initialize_orbital_from_view_axis(_view_axis)
    _orbital_active = True
    _orbital_phi -= _ORBITAL_CONFIG["movement_speed"]
    _orbital_phi = max(0.1, min(math.pi - 0.1, _orbital_phi))
    return True


def action_orbit_left_fast() -> bool:
    global _orbital_active, _orbital_theta
    if not _orbital_active:
        if _current_mesh is not None:
            _initialize_orbital_camera(_current_mesh)
        _initialize_orbital_from_view_axis(_view_axis)
    _orbital_active = True
    _orbital_theta -= (
        _ORBITAL_CONFIG["movement_speed"] * _ORBITAL_CONFIG["movement_speed_fast_multiplier"]
    )
    return True


def action_orbit_right_fast() -> bool:
    global _orbital_active, _orbital_theta
    if not _orbital_active:
        if _current_mesh is not None:
            _initialize_orbital_camera(_current_mesh)
        _initialize_orbital_from_view_axis(_view_axis)
    _orbital_active = True
    _orbital_theta += (
        _ORBITAL_CONFIG["movement_speed"] * _ORBITAL_CONFIG["movement_speed_fast_multiplier"]
    )
    return True


def action_orbit_down_fast() -> bool:
    global _orbital_active, _orbital_phi
    if not _orbital_active:
        if _current_mesh is not None:
            _initialize_orbital_camera(_current_mesh)
        _initialize_orbital_from_view_axis(_view_axis)
    _orbital_active = True
    _orbital_phi += (
        _ORBITAL_CONFIG["movement_speed"] * _ORBITAL_CONFIG["movement_speed_fast_multiplier"]
    )
    _orbital_phi = max(0.1, min(math.pi - 0.1, _orbital_phi))
    return True


def action_orbit_up_fast() -> bool:
    global _orbital_active, _orbital_phi
    if not _orbital_active:
        if _current_mesh is not None:
            _initialize_orbital_camera(_current_mesh)
        _initialize_orbital_from_view_axis(_view_axis)
    _orbital_active = True
    _orbital_phi -= (
        _ORBITAL_CONFIG["movement_speed"] * _ORBITAL_CONFIG["movement_speed_fast_multiplier"]
    )
    _orbital_phi = max(0.1, min(math.pi - 0.1, _orbital_phi))
    return True


def action_zoom_in() -> bool:
    global _orbital_active, _camera_type, _orbital_radius, _ortho_zoom
    if not _orbital_active:
        if _current_mesh is not None:
            _initialize_orbital_camera(_current_mesh)
        _initialize_orbital_from_view_axis(_view_axis)
    _orbital_active = True
    if _camera_type == "orthographic":
        _ortho_zoom *= _ORBITAL_CONFIG["zoom_in_factor"]
    else:
        _orbital_radius *= _ORBITAL_CONFIG["zoom_in_factor"]
    return True


def action_zoom_out() -> bool:
    global _orbital_active, _camera_type, _orbital_radius, _ortho_zoom
    if not _orbital_active:
        if _current_mesh is not None:
            _initialize_orbital_camera(_current_mesh)
        _initialize_orbital_from_view_axis(_view_axis)
    _orbital_active = True
    if _camera_type == "orthographic":
        _ortho_zoom *= _ORBITAL_CONFIG["zoom_out_factor"]
    else:
        _orbital_radius *= _ORBITAL_CONFIG["zoom_out_factor"]
    return True


def action_reset_orbital() -> bool:
    global _orbital_active, _orbital_theta, _orbital_phi, _orbital_radius, _orbital_initial_radius
    _orbital_active = False
    _orbital_theta = _ORBITAL_CONFIG["initial_theta"]
    _orbital_phi = _ORBITAL_CONFIG["initial_phi"]
    _orbital_radius = _orbital_initial_radius
    if _current_mesh is not None:
        _initialize_orbital_camera(_current_mesh)
    return True


# Action dispatcher
_ACTION_DISPATCH = {
    "quit": action_quit,
    "view_minus_x": action_view_minus_x,
    "view_plus_x": action_view_plus_x,
    "view_minus_y": action_view_minus_y,
    "view_plus_y": action_view_plus_y,
    "view_minus_z": action_view_minus_z,
    "view_plus_z": action_view_plus_z,
    "wireframe_off": action_wireframe_off,
    "wireframe_increase": action_wireframe_increase,
    "toggle_camera_type": action_toggle_camera_type,
    "light_decrease": action_light_decrease,
    "light_increase": action_light_increase,
    "up_vector_next": action_up_vector_next,
    "up_vector_prev": action_up_vector_prev,
    "help_toggle": action_help_toggle,
    "help_close": action_help_close,
    "orbit_left": action_orbit_left,
    "orbit_right": action_orbit_right,
    "orbit_down": action_orbit_down,
    "orbit_up": action_orbit_up,
    "orbit_left_fast": action_orbit_left_fast,
    "orbit_right_fast": action_orbit_right_fast,
    "orbit_down_fast": action_orbit_down_fast,
    "orbit_up_fast": action_orbit_up_fast,
    "zoom_in": action_zoom_in,
    "zoom_out": action_zoom_out,
    "reset_orbital": action_reset_orbital,
}

# Initialize key dispatcher
_key_dispatcher = KeyDispatcher(_KEYBINDINGS_CONFIG)


class HelpItem(TypedDict):
    actions: list[str]
    description: str


class HelpSection(TypedDict):
    title: str
    items: list[int]


# Help menu configuration
HELP_ITEMS: list[HelpItem] = [
    {"actions": ["view_minus_x", "view_plus_x"], "description": "View from -/+ X axis"},
    {"actions": ["view_minus_y", "view_plus_y"], "description": "View from -/+ Y axis"},
    {"actions": ["view_minus_z", "view_plus_z"], "description": "View from -/+ Z axis"},
    {"actions": ["up_vector_next", "up_vector_prev"], "description": "Cycle camera Up vector"},
    {"actions": ["toggle_camera_type"], "description": "Toggle Perspective/Orthographic camera"},
    {"actions": ["orbit_left", "orbit_right"], "description": "Orbit Left/Right"},
    {"actions": ["orbit_down", "orbit_up"], "description": "Orbit Down/Up"},
    {
        "actions": ["orbit_left_fast", "orbit_right_fast", "orbit_down_fast", "orbit_up_fast"],
        "description": "Fast orbit (5x speed)",
    },
    {"actions": ["zoom_in", "zoom_out"], "description": "Zoom In/Out"},
    {"actions": ["reset_orbital"], "description": "Reset camera to default view"},
    {"actions": ["light_decrease", "light_increase"], "description": "Light Intensity Down/Up"},
    {"actions": ["wireframe_increase"], "description": "Grid ON increase wireframe thikness"},
    {"actions": ["wireframe_off"], "description": "Grid OFF (solid)"},
    {"actions": ["help_toggle"], "description": "Toggle this help menu"},
    {"actions": ["quit"], "description": "Quit"},
]

HELP_SECTIONS: list[HelpSection] = [
    {"title": "Controls:", "items": [0, 1, 2, 3, 4]},
    {"title": "Orbital Camera (vim-style):", "items": [5, 6, 7, 8, 9]},
    {"title": "", "items": [10, 11, 12, 13, 14]},
]

# Camera position tracking
_camera_position: tuple[float, float, float] = (0.0, 0.0, 0.0)

# Orbital camera state (spherical coordinates)
_orbital_active = False
_orbital_theta: float = _ORBITAL_CONFIG["initial_theta"]  # Horizontal angle (azimuth)
_orbital_phi: float = _ORBITAL_CONFIG["initial_phi"]  # Vertical angle (elevation)
_orbital_radius: float = 1.0  # Distance from target (will be initialized from AABB)
_orbital_target: tuple[float, float, float] = (0.0, 0.0, 0.0)  # Mesh center (AABB center)
_orbital_initial_radius: float = 1.0  # Store initial radius for reset
_ortho_zoom: float = 1.0  # Orthographic camera zoom factor (smaller = more zoomed in)

# Lighting state
_LIGHTING_CONFIG = config.get_lighting_config()
_light_intensity: float = _LIGHTING_CONFIG["key_light_intensity"]  # Main light intensity


def _get_default_up_vector_for_axis(view_axis: str) -> tuple[float, float, float]:
    """Get the appropriate up vector for a given view axis to avoid gimbal lock.

    Args:
        view_axis: View axis ('+x', '-x', '+y', '-y', '+z', '-z')

    Returns:
        Appropriate up vector as (x, y, z)
    """
    axis = view_axis.lower()

    # For Y-axis and X-axis views, use Z as up
    if axis in ["+y", "-y", "+x", "-x"]:
        return (0.0, 0.0, 1.0)

    # For Z views (front/back), use Y as up
    return (0.0, 1.0, 0.0)


def _effective_up_vector(
    view_axis: str, up_override: tuple[float, float, float] | None
) -> tuple[float, float, float]:
    """Get the effective up vector, using config default if no override."""
    if up_override is not None:
        return up_override
    # Use first up vector from config as default
    return _UP_VECTORS[0] if _UP_VECTORS else (0.0, 1.0, 0.0)


def _format_vec3(v: tuple[float, float, float]) -> str:
    # Keep it compact for the footer.
    x, y, z = (int(v[0]), int(v[1]), int(v[2]))
    return f"({x},{y},{z})"


def _spherical_to_cartesian(
    theta: float,
    phi: float,
    radius: float,
    target: tuple[float, float, float],
    up: tuple[float, float, float],
) -> tuple[float, float, float]:
    """Convert spherical coordinates to Cartesian camera position.

    Supports Y-up and Z-up coordinate systems based on the up vector.

    Args:
        theta: Horizontal angle (azimuth) in radians
        phi: Polar angle from the up axis in radians (0 to π)
        radius: Distance from target
        target: The center point (x, y, z) to orbit around
        up: Up vector, (0,1,0) for Y-up, (0,0,1) for Z-up

    Returns:
        Camera position (x, y, z) in Cartesian coordinates
    """
    import math

    target_x, target_y, target_z = target

    sin_phi = math.sin(phi)
    cos_phi = math.cos(phi)
    cos_theta = math.cos(theta)
    sin_theta = math.sin(theta)

    if up == (0.0, 1.0, 0.0):  # Y-up
        x = target_x + radius * sin_phi * cos_theta
        y = target_y + radius * cos_phi
        z = target_z + radius * sin_phi * sin_theta
    elif up == (0.0, 0.0, 1.0):  # Z-up
        x = target_x + radius * sin_phi * cos_theta
        y = target_y + radius * sin_phi * sin_theta
        z = target_z + radius * cos_phi
    else:
        # Default to Y-up
        x = target_x + radius * sin_phi * cos_theta
        y = target_y + radius * cos_phi
        z = target_z + radius * sin_phi * sin_theta

    return (x, y, z)


def _initialize_orbital_camera(mesh: trimesh.Trimesh) -> None:
    """Initialize orbital camera from mesh AABB.

    Sets the target to the mesh center and calculates initial radius.
    """
    global _orbital_target, _orbital_radius, _orbital_initial_radius

    import numpy as np

    # Calculate AABB center
    bounds = mesh.bounds
    aabb_min = bounds[0]
    aabb_max = bounds[1]
    center = (aabb_min + aabb_max) / 2.0
    _orbital_target = tuple(center)

    # Calculate initial radius based on mesh size (same as renderer does)
    extents = aabb_max - aabb_min
    max_extent = float(np.max(extents))
    camera_cfg = config.get_camera_config()
    fov_radians = np.radians(camera_cfg["fov_degrees"])
    tan_half_fov = float(np.tan(fov_radians / 2.0))
    if not np.isfinite(tan_half_fov) or tan_half_fov <= 0.0:
        tan_half_fov = 1e-6
    distance = (max_extent / tan_half_fov) * camera_cfg["distance_padding"]
    if not np.isfinite(distance) or distance <= 0.0:
        distance = 1.0

    _orbital_radius = distance
    _orbital_initial_radius = distance


def _sync_orbital_from_camera(up: tuple[float, float, float]) -> None:
    """Synchronize orbital state from current camera position.

    Uses the specified up vector to determine the coordinate system.
    """
    global _orbital_theta, _orbital_phi, _orbital_radius

    import math

    cx, cy, cz = _camera_position
    tx, ty, tz = _orbital_target

    dx = cx - tx
    dy = cy - ty
    dz = cz - tz

    # Radius
    r = math.sqrt(dx * dx + dy * dy + dz * dz)
    if r < 1e-6:
        r = 1.0

    if up == (0.0, 1.0, 0.0):  # Y-up
        # Phi is angle from Y-axis (0 = +Y, π/2 = XZ plane, π = -Y)
        # Theta is angle in XZ plane (azimuth)
        cos_phi = max(-1.0, min(1.0, dy / r))
        phi = math.acos(cos_phi)
        theta = math.atan2(dz, dx)
    elif up == (0.0, 0.0, 1.0):  # Z-up
        # Phi is angle from Z-axis (0 = +Z, π/2 = XY plane, π = -Z)
        # Theta is angle in XY plane (azimuth)
        cos_phi = max(-1.0, min(1.0, dz / r))
        phi = math.acos(cos_phi)
        theta = math.atan2(dy, dx)
    else:
        # Default to Y-up
        cos_phi = max(-1.0, min(1.0, dy / r))
        phi = math.acos(cos_phi)
        theta = math.atan2(dz, dx)

    _orbital_radius = r
    _orbital_phi = phi
    _orbital_theta = theta


def _initialize_orbital_from_view_axis(view_axis: str) -> None:
    """Initialize orbital state from a view axis.

    Maps axis views to appropriate orbital coordinates for Y-up system.
    """
    global _orbital_theta, _orbital_phi

    import math

    axis = view_axis.lower()
    if axis == "+z":
        # Camera at +Z looking toward -Z: phi = π/2, theta = π/2
        _orbital_theta = math.pi / 2.0
        _orbital_phi = math.pi / 2.0
    elif axis == "-z":
        # Camera at -Z looking toward +Z: phi = -π/2, theta = π/2
        _orbital_theta = math.pi / 2.0
        _orbital_phi = -math.pi / 2.0
    elif axis == "+x":
        # Camera at +X looking toward -X: phi = π/2, theta = 0
        _orbital_theta = 0.0
        _orbital_phi = math.pi / 2.0
    elif axis == "-x":
        # Camera at -X looking toward +X: phi = π/2, theta = π
        _orbital_theta = math.pi
        _orbital_phi = math.pi / 2.0
    elif axis == "+y":
        # Camera at +Y looking toward -Y: phi = π/2, theta = π/2
        _orbital_theta = math.pi / 2.0
        _orbital_phi = math.pi / 2.0
    elif axis == "-y":
        # Camera at -Y looking toward +Y: phi = π/2, theta = -π/2
        _orbital_theta = -math.pi / 2.0
        _orbital_phi = math.pi / 2.0
    else:
        # Default to -z view
        _orbital_theta = -math.pi / 2.0
        _orbital_phi = math.pi / 2.0


def handle_resize(signum: int, frame: Any) -> None:
    """Signal handler for terminal resize events."""
    global _resize_pending
    _resize_pending = True


def draw_interface(cols: int, rows: int, mesh: trimesh.Trimesh | None = None) -> None:
    """Draw the TUI interface with border and footer."""
    # Top border
    print(f"\033[1;1H┌{'─' * (cols - 2)}┐", end="")

    # Side borders
    for i in range(2, rows - 1):
        print(f"\033[{i};1H│\033[{i};{cols}H│", end="")

    # Bottom border (above footer)
    print(f"\033[{rows-1};1H└{'─' * (cols - 2)}┘", end="")

    # Footer
    up_text = _format_vec3(_effective_up_vector(_view_axis, _up_vector_override))
    cam_pos_text = _format_vec3(_camera_position)

    if _camera_type == "orthographic":
        cam_type_text = "Ortho"
    else:
        cam_type_text = f"Persp ({_CAMERA_CONFIG['fov_degrees']}°)"

    left_text = (
        f" q: Quit | ?: Help | w/W: {cam_type_text} | u/U: Up {up_text} | Cam {cam_pos_text}"
    )

    right_text = ""
    if mesh is not None:
        v_count = len(mesh.vertices)
        f_count = len(mesh.faces)
        right_text = f"V: {v_count} | F: {f_count} "

    # Calculate spacing
    available_space = cols - len(left_text) - len(right_text)
    if available_space < 0:
        # If not enough space, truncate left text to fit right text
        # (Scene info is important)
        left_text = left_text[: max(0, cols - len(right_text) - 1)] + "…"
        available_space = 0

    footer_text = left_text + " " * available_space + right_text
    print(f"\033[{rows};1H{footer_text}", end="")

    # Flush
    sys.stdout.flush()


def draw_help_menu(cols: int, rows: int) -> None:
    """Draw the help menu overlay."""
    if not _show_help:
        return

    # Get background color
    bg_ansi = get_terminal_bg_ansi()
    fg_ansi = "\033[97m"  # White foreground for text
    reset_ansi = "\033[0m"

    up_text = _format_vec3(_effective_up_vector(_view_axis, _up_vector_override))

    # Generate content dynamically from config
    lines = []
    for section in HELP_SECTIONS:
        if section["title"]:
            lines.append(section["title"])
        for item_idx in section["items"]:
            item = HELP_ITEMS[item_idx]
            actions = item["actions"]
            keys = []
            for action in actions:
                keys.extend(_key_dispatcher._action_to_display_keys.get(action, []))
            if not keys:
                continue
            keys_str = "/".join(keys)
            desc = item["description"]
            if "up_vector" in actions[0]:
                desc = f"{desc} (current {up_text})"
            lines.append(f"  {keys_str} : {desc}")
        lines.append("")  # add blank after section

    # Remove trailing empty line
    if lines and lines[-1] == "":
        lines.pop()

    # Calculate menu dimensions
    menu_width = 60
    menu_height = len(lines) + 2  # +2 for borders
    start_col = (cols - menu_width) // 2
    start_row = (rows - menu_height) // 2

    # Draw each row of the menu with solid background
    for i in range(menu_height):
        row = start_row + i

        # Position cursor
        print(f"\033[{row};{start_col}H", end="")

        # Build the line content
        if i == 0:
            # Top border with title
            line_text = f"┌{'─' * (menu_width - 2)}┐"
            # Insert title in center
            title = " HELP "
            title_pos = (menu_width - len(title)) // 2
            line_text = line_text[:title_pos] + title + line_text[title_pos + len(title) :]
        elif i == menu_height - 1:
            # Bottom border
            line_text = f"└{'─' * (menu_width - 2)}┘"
        else:
            # Content line
            line_idx = i - 1
            if line_idx < len(lines):
                line = lines[line_idx]
                content = f" {line:<{menu_width - 4}} "
            else:
                content = " " * (menu_width - 2)
            line_text = f"│{content}│"

        # Print with background and foreground
        print(f"{bg_ansi}{fg_ansi}{line_text}{reset_ansi}", end="")

    sys.stdout.flush()


def render_and_display(
    mesh: trimesh.Trimesh,
    clear_screen: bool = True,
    raise_errors: bool = False,
    force_redraw: bool = False,
) -> None:
    """Render and display the mesh at current terminal size.

    Args:
        mesh: The mesh to render
        clear_screen: Whether to clear the screen before displaying
        raise_errors: Whether to raise exceptions (for initial render) or just print them
            (for resize)
        force_redraw: Force redrawing the interface (e.g., when closing help menu)
    """
    global _last_render_params, _last_image_data, _camera_position

    try:
        width_px, height_px, cell_w, cell_h = get_terminal_size()
        cols = width_px // cell_w
        rows = height_px // cell_h
    except RuntimeError as e:
        if raise_errors:
            raise
        print(f"Error: {e}", file=sys.stderr)
        return

    if clear_screen or force_redraw:
        # Clear screen when explicitly requested or when forcing redraw
        print("\033[2J", end="", flush=True)

    # Calculate inner dimensions for the mesh
    # Subtract 2 for side borders
    inner_cols = max(1, cols - 2)
    # Subtract 3 for top border (1), bottom border (1), and footer (1)
    inner_rows = max(1, rows - 3)

    inner_width_px = inner_cols * cell_w
    inner_height_px = inner_rows * cell_h

    # Apply render scale for performance
    perf_cfg = config.get_performance_config()
    render_scale = perf_cfg["render_scale"]
    render_width = max(1, int(inner_width_px * render_scale))
    render_height = max(1, int(inner_height_px * render_scale))

    try:
        # Check if we need to re-render
        current_params = {
            "mesh_id": id(mesh),
            "width": render_width,
            "height": render_height,
            "axis": _view_axis,
            "wireframe": _wireframe_thickness,
            "up": _up_vector_override,
            "camera_type": _camera_type,
            "light_intensity": _light_intensity,
            "orbital_active": _orbital_active,
            "orbital_theta": _orbital_theta,
            "orbital_phi": _orbital_phi,
            "orbital_radius": _orbital_radius,
            "ortho_zoom": _ortho_zoom,
        }

        if current_params != _last_render_params or _last_image_data is None:
            # Use orbital camera if active
            if _orbital_active:
                # Use the current up vector override if set, otherwise default to Y-up
                effective_up = (
                    _up_vector_override if _up_vector_override is not None else (0.0, 1.0, 0.0)
                )
                eye = _spherical_to_cartesian(
                    _orbital_theta, _orbital_phi, _orbital_radius, _orbital_target, effective_up
                )
                image_data, cam_pos = render_mesh(
                    mesh,
                    render_width,
                    render_height,
                    view_axis=_view_axis,
                    wireframe_thickness=_wireframe_thickness,
                    up_vector_override=effective_up,
                    orbital_eye=eye,
                    orbital_target=_orbital_target,
                    camera_type=_camera_type,
                    light_intensity=_light_intensity,
                    ortho_zoom=_ortho_zoom,
                )
            else:
                # For axis views, use the appropriate up vector
                effective_up = _effective_up_vector(_view_axis, _up_vector_override)
                image_data, cam_pos = render_mesh(
                    mesh,
                    render_width,
                    render_height,
                    view_axis=_view_axis,
                    wireframe_thickness=_wireframe_thickness,
                    up_vector_override=effective_up,
                    camera_type=_camera_type,
                    light_intensity=_light_intensity,
                    ortho_zoom=_ortho_zoom,
                )
            _last_image_data = image_data
            _camera_position = cam_pos
            _last_render_params = current_params
        else:
            image_data = _last_image_data

        # Draw interface (border and footer) - AFTER rendering to get updated camera position
        draw_interface(cols, rows, mesh)

        if _show_help:
            # When showing help, delete images and draw menu
            clear_images()
            draw_help_menu(cols, rows)
        else:
            # Move cursor to inside top-left (row 2, col 2)
            print("\033[2;2H", end="", flush=True)
            # Display the image (overwriting existing image with ID 1)
            # Note: render_width/height may be scaled down, display_image will handle upscaling
            display_image(
                image_data,
                render_width,
                render_height,
                cols=inner_cols,
                rows=inner_rows,
                image_id=1,
            )

    except Exception as e:
        if raise_errors:
            raise
        print(f"Error rendering/displaying: {e}", file=sys.stderr)


def wait_for_exit() -> None:
    """Wait for user input to control the viewer.

    Handles:
    - q: Quit
    - x/X, y/Y, z/Z: Change view axis
    - g: Toggle grid/wireframe
    - h/j/k/l: Orbital camera (vim-style)
    - H/J/K/L: Fast orbital camera
    - r/R: Zoom in/out
    - 0: Reset orbital camera
    - ?: Toggle help
    - Resize events
    """
    global _resize_pending
    global _show_help
    global _up_vector_cycle_index
    global _up_vector_override
    global _view_axis
    global _wireframe_thickness
    global _camera_type
    global _light_intensity
    global _orbital_active
    global _orbital_theta
    global _orbital_phi
    global _orbital_radius
    global _ortho_zoom

    if not sys.stdin.isatty():
        return

    try:
        fd = sys.stdin.fileno()
        old_settings = termios.tcgetattr(fd)
        try:
            tty.setraw(fd)

            while True:
                # Check if resize is pending
                if _resize_pending and _current_mesh is not None:
                    _resize_pending = False
                    render_and_display(_current_mesh, clear_screen=True)

                # Check for input with timeout to allow resize handling
                import select

                # Drain input buffer to prevent lag
                needs_rerender: bool | str = False
                while select.select([sys.stdin], [], [], 0.0)[0]:
                    char = sys.stdin.read(1)

                    # Dispatch key to action
                    action = _key_dispatcher.get_action(char)
                    if action and action in _ACTION_DISPATCH:
                        result = _ACTION_DISPATCH[action]()
                        if result:
                            needs_rerender = result

                if needs_rerender and _current_mesh is not None:
                    # Handle force redraw (e.g., closing help menu)
                    if needs_rerender == "force_redraw":
                        render_and_display(_current_mesh, clear_screen=False, force_redraw=True)
                    else:
                        # Don't clear screen on interactive updates to avoid flickering
                        render_and_display(_current_mesh, clear_screen=False)

                # Sleep briefly to prevent CPU spinning if no input
                import time

                time.sleep(0.01)

        finally:
            termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)
    except (OSError, termios.error):
        # If we can't set up raw mode, just wait for Enter
        input()

    print("\r" + " " * 70 + "\r", end="", flush=True)  # Clear the line


def setup_tui() -> None:
    """Enter TUI mode: alternate screen buffer, hidden cursor."""
    sys.stdout.write("\033[?1049h")  # Enter alternate screen buffer
    sys.stdout.write("\033[?25l")  # Hide cursor
    sys.stdout.flush()


def cleanup_tui() -> None:
    """Exit TUI mode: exit alternate screen buffer, show cursor."""
    sys.stdout.write("\033[?25h")  # Show cursor
    sys.stdout.write("\033[?1049l")  # Exit alternate screen buffer
    sys.stdout.flush()


def main() -> int:
    """Main entry point for meshtui CLI.

    Loads a mesh file, renders it, and displays it in the terminal.
    Automatically rerenders on terminal resize.

    Returns:
        Exit code (0 for success, non-zero for error)
    """
    global _current_mesh

    # Parse arguments
    if len(sys.argv) < 2:
        print("Usage: meshtui <mesh_file>", file=sys.stderr)
        print("\nSupported formats: .ply, .stl, .obj, .drc, .glb", file=sys.stderr)
        return 1

    mesh_path = Path(sys.argv[1])

    try:
        # Check terminal compatibility
        try:
            get_terminal_size()
        except RuntimeError as e:
            print(f"Error: {e}", file=sys.stderr)
            return 1

        # Load mesh
        print(f"Loading mesh: {mesh_path}")
        try:
            mesh = load_mesh(mesh_path)
            _current_mesh = mesh  # Store for resize handling
            # Initialize orbital camera from mesh
            _initialize_orbital_camera(mesh)
        except FileNotFoundError:
            print(f"Error: File not found: {mesh_path}", file=sys.stderr)
            return 1
        except ValueError as e:
            print(f"Error: {e}", file=sys.stderr)
            return 1

        # Set up signal handler for terminal resize
        signal.signal(signal.SIGWINCH, handle_resize)

        # Enter TUI mode
        setup_tui()

        # Initial render and display (raise errors for initial render)
        try:
            render_and_display(mesh, clear_screen=True, raise_errors=True)
        except Exception as e:
            # If render fails, we still need to cleanup TUI
            cleanup_tui()
            print(f"Error rendering/displaying: {e}", file=sys.stderr)
            return 1

        # Wait for user to exit (with resize handling)
        wait_for_exit()

        return 0

    except KeyboardInterrupt:
        return 0
    except Exception as e:
        cleanup_tui()
        print(f"Unexpected error: {e}", file=sys.stderr)
        return 1
    finally:
        # Clean up global state and TUI
        _current_mesh = None
        cleanup_tui()


if __name__ == "__main__":
    sys.exit(main())
