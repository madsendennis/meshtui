"""Main entry point for meshtui CLI."""

import math
import signal
import sys
import termios
import tty
from pathlib import Path
from typing import Any

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
_VIEW_CONFIG = config.get_view_config()
_WIREFRAME_CONFIG = config.get_wireframe_config()
_ORBITAL_CONFIG = config.get_orbital_camera_config()

# Global state for handling terminal resize
_current_mesh: trimesh.Trimesh | None = None
_resize_pending = False
_view_axis = _VIEW_CONFIG["default_axis"]
_wireframe_thickness = _WIREFRAME_CONFIG["default_thickness"]
_show_help = False
_up_vector_cycle_index = -1  # -1 means use default (index 0)
_up_vector_override: tuple[float, float, float] | None = None
_last_render_params: dict[str, Any] = {}
_last_image_data: bytes | None = None


# Load up vector list from config (index 0 is the default)
_UP_VECTORS: list[tuple[float, float, float]] = [tuple(v) for v in _VIEW_CONFIG["up_vectors"]]

# Camera position tracking
_camera_position: tuple[float, float, float] = (0.0, 0.0, 0.0)

# Orbital camera state (spherical coordinates)
_orbital_active = False
_orbital_theta: float = _ORBITAL_CONFIG["initial_theta"]  # Horizontal angle (azimuth)
_orbital_phi: float = _ORBITAL_CONFIG["initial_phi"]  # Vertical angle (elevation)
_orbital_radius: float = 1.0  # Distance from target (will be initialized from AABB)
_orbital_target: tuple[float, float, float] = (0.0, 0.0, 0.0)  # Mesh center (AABB center)
_orbital_initial_radius: float = 1.0  # Store initial radius for reset


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
    theta: float, phi: float, radius: float, target: tuple[float, float, float]
) -> tuple[float, float, float]:
    """Convert spherical coordinates to Cartesian camera position.

    Args:
        theta: Horizontal angle (azimuth) in radians
        phi: Vertical angle (elevation) in radians
        radius: Distance from target
        target: The center point (x, y, z) to orbit around

    Returns:
        Camera position (x, y, z) in Cartesian coordinates
    """
    import math

    target_x, target_y, target_z = target

    # Spherical to Cartesian conversion
    x = target_x + radius * math.sin(phi) * math.cos(theta)
    y = target_y + radius * math.cos(phi)
    z = target_z + radius * math.sin(phi) * math.sin(theta)

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
    cam_text = _format_vec3(_camera_position)
    left_text = f" q: Quit | ?: Help | u/U: Up {up_text} | Cam {cam_text}"

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

    # Content
    lines = [
        "Controls:",
        "  x/X : View from +/- X axis",
        "  y/Y : View from +/- Y axis",
        "  z/Z : View from +/- Z axis",
        f"  u/U : Cycle camera Up vector (current {up_text})",
        "",
        "Orbital Camera (vim-style):",
        "  h/l : Orbit Left/Right",
        "  j/k : Orbit Down/Up",
        "  H/L/J/K : Fast orbit (5x speed)",
        "  r/R : Zoom In/Out",
        "  0   : Reset camera to default view",
        "",
        "  G   : Grid ON (wireframe)",
        "  g   : Grid OFF (solid)",
        "  ?   : Toggle this help menu",
        "  q   : Quit",
    ]

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
    mesh: trimesh.Trimesh, clear_screen: bool = True, raise_errors: bool = False
) -> None:
    """Render and display the mesh at current terminal size.

    Args:
        mesh: The mesh to render
        clear_screen: Whether to clear the screen before displaying
        raise_errors: Whether to raise exceptions (for initial render) or just print them
            (for resize)
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

    if clear_screen:
        # Clear screen
        print("\033[2J", end="", flush=True)

    # Draw interface (border and footer)
    draw_interface(cols, rows, mesh)

    # Calculate inner dimensions for the mesh
    # Subtract 2 for side borders
    inner_cols = max(1, cols - 2)
    # Subtract 3 for top border (1), bottom border (1), and footer (1)
    inner_rows = max(1, rows - 3)

    inner_width_px = inner_cols * cell_w
    inner_height_px = inner_rows * cell_h

    try:
        # Check if we need to re-render
        current_params = {
            "mesh_id": id(mesh),
            "width": inner_width_px,
            "height": inner_height_px,
            "axis": _view_axis,
            "wireframe": _wireframe_thickness,
            "up": _up_vector_override,
            "orbital_active": _orbital_active,
            "orbital_theta": _orbital_theta,
            "orbital_phi": _orbital_phi,
            "orbital_radius": _orbital_radius,
        }

        if current_params != _last_render_params or _last_image_data is None:
            # Always pass effective up vector (config default or user-cycled)
            effective_up = _effective_up_vector(_view_axis, _up_vector_override)

            # Use orbital camera if active
            if _orbital_active:
                eye = _spherical_to_cartesian(
                    _orbital_theta, _orbital_phi, _orbital_radius, _orbital_target
                )
                image_data, cam_pos = render_mesh(
                    mesh,
                    inner_width_px,
                    inner_height_px,
                    view_axis=_view_axis,
                    wireframe_thickness=_wireframe_thickness,
                    up_vector_override=effective_up,
                    orbital_eye=eye,
                    orbital_target=_orbital_target,
                )
            else:
                image_data, cam_pos = render_mesh(
                    mesh,
                    inner_width_px,
                    inner_height_px,
                    view_axis=_view_axis,
                    wireframe_thickness=_wireframe_thickness,
                    up_vector_override=effective_up,
                )
            _last_image_data = image_data
            _camera_position = cam_pos
            _last_render_params = current_params
        else:
            image_data = _last_image_data

        if _show_help:
            # When showing help, delete images and draw menu
            clear_images()
            draw_help_menu(cols, rows)
        else:
            # Move cursor to inside top-left (row 2, col 2)
            print("\033[2;2H", end="", flush=True)
            # Display the image
            display_image(
                image_data,
                inner_width_px,
                inner_height_px,
                cols=inner_cols,
                rows=inner_rows,
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
    global _orbital_active
    global _orbital_theta
    global _orbital_phi
    global _orbital_radius

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

                if select.select([sys.stdin], [], [], 0.1)[0]:
                    char = sys.stdin.read(1)

                    needs_rerender = False

                    if char == "q":
                        break
                    elif char == "x":
                        _view_axis = "+x"
                        # Update orbital camera to match +X view
                        _orbital_theta = 0.0
                        _orbital_phi = math.pi / 2.0
                        needs_rerender = True
                    elif char == "X":
                        _view_axis = "-x"
                        # Update orbital camera to match -X view
                        _orbital_theta = math.pi
                        _orbital_phi = math.pi / 2.0
                        needs_rerender = True
                    elif char == "y":
                        _view_axis = "+y"
                        # Update orbital camera to match +Y view (top view)
                        _orbital_theta = 0.0
                        _orbital_phi = 0.1  # Clamped to avoid gimbal lock
                        needs_rerender = True
                    elif char == "Y":
                        _view_axis = "-y"
                        # Update orbital camera to match -Y view (bottom view)
                        _orbital_theta = 0.0
                        _orbital_phi = math.pi - 0.1  # Clamped to avoid gimbal lock
                        needs_rerender = True
                    elif char == "z":
                        _view_axis = "+z"
                        # Update orbital camera to match +Z view
                        _orbital_theta = math.pi / 2.0
                        _orbital_phi = math.pi / 2.0
                        needs_rerender = True
                    elif char == "Z":
                        _view_axis = "-z"
                        # Update orbital camera to match -Z view
                        _orbital_theta = -math.pi / 2.0
                        _orbital_phi = math.pi / 2.0
                        needs_rerender = True
                    elif char == "G":
                        if _wireframe_thickness == 0.0:
                            _wireframe_thickness = 1.0
                        else:
                            _wireframe_thickness += 1.0
                        needs_rerender = True
                    elif char == "g":
                        _wireframe_thickness = 0.0
                        needs_rerender = True
                    elif char == "u":
                        if _up_vector_cycle_index == -1:
                            _up_vector_cycle_index = 0
                        else:
                            _up_vector_cycle_index = (_up_vector_cycle_index + 1) % len(_UP_VECTORS)
                        _up_vector_override = _UP_VECTORS[_up_vector_cycle_index]
                        needs_rerender = True
                    elif char == "U":
                        if _up_vector_cycle_index == -1:
                            _up_vector_cycle_index = len(_UP_VECTORS) - 1
                        else:
                            _up_vector_cycle_index = (_up_vector_cycle_index - 1) % len(_UP_VECTORS)
                        _up_vector_override = _UP_VECTORS[_up_vector_cycle_index]
                        needs_rerender = True
                    elif char == "?":
                        _show_help = not _show_help
                        needs_rerender = True
                    elif char == "\x1b" and _show_help:  # Esc
                        _show_help = False
                        needs_rerender = True
                    # Orbital camera controls
                    elif char == "h":  # Orbit left (decrease theta)
                        _orbital_active = True
                        _orbital_theta -= _ORBITAL_CONFIG["movement_speed"]
                        needs_rerender = True
                    elif char == "l":  # Orbit right (increase theta)
                        _orbital_active = True
                        _orbital_theta += _ORBITAL_CONFIG["movement_speed"]
                        needs_rerender = True
                    elif char == "j":  # Orbit down (increase phi)
                        _orbital_active = True
                        _orbital_phi += _ORBITAL_CONFIG["movement_speed"]
                        # Clamp phi to avoid gimbal lock
                        _orbital_phi = max(0.1, min(math.pi - 0.1, _orbital_phi))
                        needs_rerender = True
                    elif char == "k":  # Orbit up (decrease phi)
                        _orbital_active = True
                        _orbital_phi -= _ORBITAL_CONFIG["movement_speed"]
                        # Clamp phi to avoid gimbal lock
                        _orbital_phi = max(0.1, min(math.pi - 0.1, _orbital_phi))
                        needs_rerender = True
                    elif char == "H":  # Orbit left fast
                        _orbital_active = True
                        _orbital_theta -= (
                            _ORBITAL_CONFIG["movement_speed"]
                            * _ORBITAL_CONFIG["movement_speed_fast_multiplier"]
                        )
                        needs_rerender = True
                    elif char == "L":  # Orbit right fast
                        _orbital_active = True
                        _orbital_theta += (
                            _ORBITAL_CONFIG["movement_speed"]
                            * _ORBITAL_CONFIG["movement_speed_fast_multiplier"]
                        )
                        needs_rerender = True
                    elif char == "J":  # Orbit down fast
                        _orbital_active = True
                        _orbital_phi += (
                            _ORBITAL_CONFIG["movement_speed"]
                            * _ORBITAL_CONFIG["movement_speed_fast_multiplier"]
                        )
                        # Clamp phi to avoid gimbal lock
                        _orbital_phi = max(0.1, min(math.pi - 0.1, _orbital_phi))
                        needs_rerender = True
                    elif char == "K":  # Orbit up fast
                        _orbital_active = True
                        _orbital_phi -= (
                            _ORBITAL_CONFIG["movement_speed"]
                            * _ORBITAL_CONFIG["movement_speed_fast_multiplier"]
                        )
                        # Clamp phi to avoid gimbal lock
                        _orbital_phi = max(0.1, min(math.pi - 0.1, _orbital_phi))
                        needs_rerender = True
                    elif char == "r":  # Zoom in
                        _orbital_active = True
                        _orbital_radius *= _ORBITAL_CONFIG["zoom_in_factor"]
                        needs_rerender = True
                    elif char == "R":  # Zoom out
                        _orbital_active = True
                        _orbital_radius *= _ORBITAL_CONFIG["zoom_out_factor"]
                        needs_rerender = True
                    elif char == "0":  # Reset orbital camera
                        _orbital_active = False
                        _orbital_theta = _ORBITAL_CONFIG["initial_theta"]
                        _orbital_phi = _ORBITAL_CONFIG["initial_phi"]
                        _orbital_radius = _orbital_initial_radius
                        # Re-initialize target from current mesh
                        if _current_mesh is not None:
                            _initialize_orbital_camera(_current_mesh)
                        needs_rerender = True

                    if needs_rerender and _current_mesh is not None:
                        render_and_display(_current_mesh, clear_screen=True)

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
        print("\nSupported formats: .ply, .stl", file=sys.stderr)
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
