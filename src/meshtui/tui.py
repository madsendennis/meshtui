import select
import signal
import sys
import termios
import tty
from collections import defaultdict
from contextlib import suppress
from typing import Any, TypedDict

import numpy as np
import trimesh

from meshtui import config
from meshtui.camera import Camera, OrthographicCamera, PerspectiveCamera
from meshtui.camera_setup import calculate_camera_distance, calculate_camera_parameters
from meshtui.kitty_protocol import (
    clear_images,
    display_image,
    get_terminal_bg_ansi,
    get_terminal_size,
)
from meshtui.renderer import render_mesh
from meshtui.screenshot import save_screenshot

# Special key mappings for raw terminal input
_SPECIAL_KEYS = {
    "esc": "\x1b",
    "space": " ",
    "tab": "\t",
    "enter": "\r",
    "backspace": "\x7f",
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
    {"actions": ["screenshot"], "description": "Save screenshot (PNG)"},
    {"actions": ["help_toggle"], "description": "Toggle this help menu"},
    {"actions": ["quit"], "description": "Quit"},
]

HELP_SECTIONS: list[HelpSection] = [
    {"title": "Controls:", "items": [0, 1, 2, 3, 4]},
    {"title": "Orbital Camera (vim-style):", "items": [5, 6, 7, 8, 9]},
    {"title": "", "items": [10, 11, 12, 13, 14, 15]},
]


class TUI:
    def __init__(self, meshes: list[trimesh.Trimesh] | trimesh.Trimesh):
        if isinstance(meshes, trimesh.Trimesh):
            self.meshes = [meshes]
        else:
            self.meshes = meshes

        self.resize_pending = False
        self.show_help = False

        # Config
        self.view_config = config.get_view_config()
        self.wireframe_config = config.get_wireframe_config()
        self.camera_config = config.get_camera_config()
        self.orbital_config = config.get_orbital_camera_config()
        self.lighting_config = config.get_lighting_config()
        self.keybindings_config = config.get_keybindings_config()

        # State
        self.wireframe_thickness = self.wireframe_config["default_thickness"]
        self.light_intensity = self.lighting_config["key_light_intensity"]
        self.view_axis = self.view_config["default_axis"]

        # Up vectors
        self.up_vectors: list[tuple[float, float, float]] = [
            tuple(v) for v in self.view_config["up_vectors"]
        ]
        self.up_vector_cycle_index = -1
        self.up_vector_override: tuple[float, float, float] | None = None

        # Camera
        self.camera: Camera
        if self.camera_config["type"] == "orthographic":
            self.camera = OrthographicCamera()
        else:
            self.camera = PerspectiveCamera()

        self._initialize_camera()

        # Input
        self.key_dispatcher = KeyDispatcher(self.keybindings_config)

        # Rendering state
        self.last_render_params: dict[str, Any] = {}
        self.last_image_data: bytes | None = None
        self.last_render_width: int = 0
        self.last_render_height: int = 0
        self.camera_position = (0.0, 0.0, 0.0)

    def _initialize_camera(self):
        if not self.meshes:
            # Fallback for empty mesh list
            center = np.array([0.0, 0.0, 0.0])
            max_extent = 1.0
        else:
            # Use shared camera setup logic
            center, max_extent = calculate_camera_parameters(
                self.meshes, self.camera_config["distance_padding"]
            )

        self.camera.set_target(tuple(center))

        # Calculate camera distance based on camera type
        distance = calculate_camera_distance(
            max_extent,
            self.camera_config["distance_padding"],
            self.camera_config["fov_degrees"],
            self.camera.get_type(),
        )

        self.camera.set_radius(distance)
        self.camera.set_view_axis(self.view_axis)

    def run(self):
        # Set up signal handler
        signal.signal(signal.SIGWINCH, self._handle_resize)

        self._setup_tui()
        try:
            self.render_and_display(clear_screen=True, raise_errors=True)
            self._wait_for_exit()
        except Exception as e:
            self._cleanup_tui()
            print(f"Error: {e}", file=sys.stderr)
            sys.exit(1)
        finally:
            self._cleanup_tui()

    def _handle_resize(self, signum, frame):
        self.resize_pending = True

    def _setup_tui(self):
        sys.stdout.write("\033[?1049h")  # Enter alternate screen buffer
        sys.stdout.write("\033[?25l")  # Hide cursor
        sys.stdout.flush()

    def _cleanup_tui(self):
        sys.stdout.write("\033[?25h")  # Show cursor
        sys.stdout.write("\033[?1049l")  # Exit alternate screen buffer
        sys.stdout.flush()

    def _wait_for_exit(self):
        if not sys.stdin.isatty():
            return

        try:
            fd = sys.stdin.fileno()
            old_settings = termios.tcgetattr(fd)
            try:
                tty.setraw(fd)
                while True:
                    if self.resize_pending:
                        self.resize_pending = False
                        self.render_and_display(clear_screen=True)

                    # Animation step
                    smoothing = self.orbital_config.get("smoothing_factor", 0.6)
                    is_animating = self.camera.animate(smoothing)

                    if is_animating:
                        self.render_and_display(clear_screen=False)
                        timeout = 0.0  # Don't block if animating
                    else:
                        timeout = 0.1  # Block briefly if idle

                    # Use a timeout to allow checking for resize events if select is not interrupted
                    try:
                        rlist, _, _ = select.select([sys.stdin], [], [], timeout)
                    except OSError:
                        # Likely interrupted by signal (resize)
                        continue

                    if rlist:
                        char = sys.stdin.read(1)
                        action = self.key_dispatcher.get_action(char)
                        if action:
                            should_redraw = self._handle_action(action)
                            if should_redraw:
                                self.render_and_display(
                                    clear_screen=False,
                                    force_redraw=(should_redraw == "force_redraw"),
                                )

            finally:
                termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)
        except (OSError, termios.error):
            pass

    def _handle_action(self, action: str) -> bool | str:
        if action == "quit":
            sys.exit(0)
        elif action == "help_toggle":
            self.show_help = not self.show_help
            return True
        elif action == "help_close":
            if self.show_help:
                self.show_help = False
                return "force_redraw"
            return False

        # Camera actions
        if action.startswith("view_"):
            axis = action.replace("view_", "").replace("plus_", "+").replace("minus_", "-")
            self.camera.set_view_axis(axis)  # type: ignore
            self.view_axis = axis
            return True

        if action == "toggle_camera_type":
            # Switch camera instance but preserve state where possible
            target = self.camera.target
            radius = self.camera.radius
            target_radius = self.camera.target_radius
            theta = self.camera.theta
            target_theta = self.camera.target_theta
            phi = self.camera.phi
            target_phi = self.camera.target_phi
            up = self.camera.up_vector

            if isinstance(self.camera, PerspectiveCamera):
                self.camera = OrthographicCamera(target)
            else:
                self.camera = PerspectiveCamera(target)

            self.camera.radius = radius
            self.camera.target_radius = target_radius
            self.camera.theta = theta
            self.camera.target_theta = target_theta
            self.camera.phi = phi
            self.camera.target_phi = target_phi
            self.camera.up_vector = up
            self.camera.update_position()
            return True

        if action == "wireframe_off":
            self.wireframe_thickness = 0.0
            return True
        if action == "wireframe_increase":
            if self.wireframe_thickness == 0.0:
                self.wireframe_thickness = 1.0
            else:
                self.wireframe_thickness += 1.0
            return True

        if action == "light_decrease":
            self.light_intensity = max(0.1, self.light_intensity - 0.5)
            return True
        if action == "light_increase":
            self.light_intensity = min(10.0, self.light_intensity + 0.5)
            return True

        if action == "up_vector_next":
            if self.up_vector_cycle_index == -1:
                self.up_vector_cycle_index = 0
            else:
                self.up_vector_cycle_index = (self.up_vector_cycle_index + 1) % len(self.up_vectors)
            self.up_vector_override = self.up_vectors[self.up_vector_cycle_index]
            self.camera.up_vector = self.up_vector_override
            self.camera.update_position()
            return True

        if action == "up_vector_prev":
            if self.up_vector_cycle_index == -1:
                self.up_vector_cycle_index = len(self.up_vectors) - 1
            else:
                self.up_vector_cycle_index = (self.up_vector_cycle_index - 1) % len(self.up_vectors)
            self.up_vector_override = self.up_vectors[self.up_vector_cycle_index]
            self.camera.up_vector = self.up_vector_override
            self.camera.update_position()
            return True

        # Orbital controls
        speed = self.orbital_config["movement_speed"]
        fast_mult = self.orbital_config["movement_speed_fast_multiplier"]

        if action == "orbit_left":
            self.camera.orbit(-speed, 0)
            return True
        if action == "orbit_right":
            self.camera.orbit(speed, 0)
            return True
        if action == "orbit_down":
            self.camera.orbit(0, speed)
            return True
        if action == "orbit_up":
            self.camera.orbit(0, -speed)
            return True

        if action == "orbit_left_fast":
            self.camera.orbit(-speed * fast_mult, 0)
            return True
        if action == "orbit_right_fast":
            self.camera.orbit(speed * fast_mult, 0)
            return True
        if action == "orbit_down_fast":
            self.camera.orbit(0, speed * fast_mult)
            return True
        if action == "orbit_up_fast":
            self.camera.orbit(0, -speed * fast_mult)
            return True

        if action == "zoom_in":
            self.camera.zoom(self.orbital_config["zoom_out_factor"])  # Zoom in reduces radius/fov
            return True
        if action == "zoom_out":
            self.camera.zoom(self.orbital_config["zoom_in_factor"])
            return True

        if action == "reset_orbital":
            self._initialize_camera()
            return True

        if action == "screenshot":
            self._save_screenshot()
            return False  # No need to redraw after screenshot

        return False

    def render_and_display(self, clear_screen=True, raise_errors=False, force_redraw=False):
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
            print("\033[2J", end="", flush=True)

        inner_cols = max(1, cols - 2)
        inner_rows = max(1, rows - 3)
        inner_width_px = inner_cols * cell_w
        inner_height_px = inner_rows * cell_h

        perf_cfg = config.get_performance_config()
        render_scale = perf_cfg["render_scale"]
        render_width = max(1, int(inner_width_px * render_scale))
        render_height = max(1, int(inner_height_px * render_scale))

        try:
            # Check if we need to re-render
            current_params = {
                "mesh_ids": tuple(id(m) for m in self.meshes),
                "width": render_width,
                "height": render_height,
                "wireframe": self.wireframe_thickness,
                "camera_type": self.camera.get_type(),
                "light_intensity": self.light_intensity,
                "camera_pos": self.camera.position,
                "camera_target": self.camera.target,
                "camera_up": self.camera.up_vector,
                "ortho_zoom": (
                    self.camera.zoom_level if isinstance(self.camera, OrthographicCamera) else 1.0
                ),
            }

            if current_params != self.last_render_params or self.last_image_data is None:
                image_data, cam_pos = render_mesh(
                    self.meshes,
                    render_width,
                    render_height,
                    view_axis=self.view_axis,  # Still needed for some renderer logic?
                    wireframe_thickness=self.wireframe_thickness,
                    up_vector_override=self.camera.up_vector,
                    orbital_eye=self.camera.position,
                    orbital_target=tuple(self.camera.target),
                    camera_type=self.camera.get_type(),
                    light_intensity=self.light_intensity,
                    ortho_zoom=(
                        self.camera.zoom_level
                        if isinstance(self.camera, OrthographicCamera)
                        else 1.0
                    ),
                )
                self.last_image_data = image_data
                self.last_render_width = render_width
                self.last_render_height = render_height
                self.camera_position = cam_pos
                self.last_render_params = current_params
            else:
                image_data = self.last_image_data

            self._draw_interface(cols, rows)

            if self.show_help:
                clear_images()
                self._draw_help_menu(cols, rows)
            else:
                print("\033[2;2H", end="", flush=True)
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

    def _draw_interface(self, cols: int, rows: int):
        # Top border
        print(f"\033[1;1H┌{'─' * (cols - 2)}┐", end="")

        # Side borders
        for i in range(2, rows - 1):
            print(f"\033[{i};1H│\033[{i};{cols}H│", end="")

        # Bottom border (above footer)
        print(f"\033[{rows-1};1H└{'─' * (cols - 2)}┘", end="")

        # Footer
        def format_vec3(v):
            x, y, z = (int(v[0]), int(v[1]), int(v[2]))
            return f"({x},{y},{z})"

        up_text = format_vec3(self.camera.up_vector)
        cam_pos_text = format_vec3(self.camera.position)

        if isinstance(self.camera, OrthographicCamera):
            cam_type_text = "Ortho"
        else:
            cam_type_text = f"Persp ({self.camera_config['fov_degrees']}°)"

        left_text = (
            f" q: Quit | ?: Help | w/W: {cam_type_text} | u/U: Up {up_text} | Cam {cam_pos_text}"
        )

        right_text = ""
        if self.meshes:
            v_count = sum(len(m.vertices) for m in self.meshes)
            f_count = sum(len(m.faces) for m in self.meshes)
            mesh_count = len(self.meshes)
            if mesh_count > 1:
                right_text = f"M: {mesh_count} | V: {v_count} | F: {f_count} "
            else:
                right_text = f"V: {v_count} | F: {f_count} "

        available_space = cols - len(left_text) - len(right_text)
        if available_space < 0:
            left_text = left_text[: max(0, cols - len(right_text) - 1)] + "…"
            available_space = 0

        footer_text = left_text + " " * available_space + right_text
        print(f"\033[{rows};1H{footer_text}", end="")
        sys.stdout.flush()

    def _save_screenshot(self):
        """Save the current rendered image as a PNG screenshot."""
        if self.last_image_data is None:
            return

        with suppress(Exception):
            save_screenshot(self.last_image_data, self.last_render_width, self.last_render_height)

    def _draw_help_menu(self, cols: int, rows: int):
        bg_ansi = get_terminal_bg_ansi()
        fg_ansi = "\033[97m"
        reset_ansi = "\033[0m"

        def format_vec3(v):
            x, y, z = (int(v[0]), int(v[1]), int(v[2]))
            return f"({x},{y},{z})"

        up_text = format_vec3(self.camera.up_vector)

        lines = []
        for section in HELP_SECTIONS:
            if section["title"]:
                lines.append(section["title"])
            for item_idx in section["items"]:
                item = HELP_ITEMS[item_idx]
                actions = item["actions"]
                keys = []
                for action in actions:
                    keys.extend(self.key_dispatcher._action_to_display_keys.get(action, []))
                if not keys:
                    continue
                keys_str = "/".join(keys)
                desc = item["description"]
                if "up_vector" in actions[0]:
                    desc = f"{desc} (current {up_text})"
                lines.append(f"  {keys_str} : {desc}")
            lines.append("")

        if lines and lines[-1] == "":
            lines.pop()

        menu_width = 60
        menu_height = len(lines) + 2
        start_col = (cols - menu_width) // 2
        start_row = (rows - menu_height) // 2

        for i in range(menu_height):
            row = start_row + i
            print(f"\033[{row};{start_col}H", end="")

            if i == 0:
                line_text = f"┌{'─' * (menu_width - 2)}┐"
                title = " HELP "
                title_pos = (menu_width - len(title)) // 2
                line_text = line_text[:title_pos] + title + line_text[title_pos + len(title) :]
            elif i == menu_height - 1:
                line_text = f"└{'─' * (menu_width - 2)}┘"
            else:
                line_idx = i - 1
                if line_idx < len(lines):
                    line = lines[line_idx]
                    content = f" {line:<{menu_width - 4}} "
                else:
                    content = " " * (menu_width - 2)
                line_text = f"│{content}│"

            print(f"{bg_ansi}{fg_ansi}{line_text}{reset_ansi}", end="")

        sys.stdout.flush()
