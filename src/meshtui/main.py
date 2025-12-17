"""Main entry point for meshtui CLI."""

import signal
import sys
import termios
import tty
from pathlib import Path
from typing import Any

import trimesh

from meshtui.kitty_protocol import (
    clear_images,
    display_image,
    get_terminal_bg_ansi,
    get_terminal_size,
)
from meshtui.mesh_loader import load_mesh
from meshtui.renderer import render_mesh

# Global state for handling terminal resize
_current_mesh: trimesh.Trimesh | None = None
_resize_pending = False
_view_axis = "+z"
_wireframe_thickness = 0.0
_show_help = False
_up_vector_cycle_index = -1
_up_vector_override: tuple[float, float, float] | None = None
_last_render_params: dict[str, Any] = {}
_last_image_data: bytes | None = None


_UP_VECTORS: list[tuple[float, float, float]] = [
    (0.0, 1.0, 0.0),
    (0.0, -1.0, 0.0),
    (0.0, 0.0, 1.0),
    (0.0, 0.0, -1.0),
]


def _effective_up_vector(
    view_axis: str, up_override: tuple[float, float, float] | None
) -> tuple[float, float, float]:
    if up_override is not None:
        return up_override

    axis = view_axis.lower()
    if axis == "+y":
        return (0.0, 0.0, -1.0)
    if axis == "-y":
        return (0.0, 0.0, 1.0)
    return (0.0, 1.0, 0.0)


def _format_vec3(v: tuple[float, float, float]) -> str:
    # Keep it compact for the footer.
    x, y, z = (int(v[0]), int(v[1]), int(v[2]))
    return f"({x},{y},{z})"


def handle_resize(signum: int, frame: Any) -> None:
    """Signal handler for terminal resize events."""
    global _resize_pending
    _resize_pending = True


def draw_interface(cols: int, rows: int) -> None:
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
    footer_text = f" q: Quit | ?: Help | u/U: Up {up_text}"
    # Pad footer with spaces to clear line
    padding = " " * max(0, cols - len(footer_text))
    print(f"\033[{rows};1H{footer_text}{padding}", end="")

    # Flush
    sys.stdout.flush()


def draw_help_menu(cols: int, rows: int) -> None:
    """Draw the help menu overlay."""
    if not _show_help:
        return

    # Calculate menu dimensions (larger size)
    menu_width = 60
    menu_height = 16
    start_col = (cols - menu_width) // 2
    start_row = (rows - menu_height) // 2

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
        "  G   : Grid ON (wireframe)",
        "  g   : Grid OFF (solid)",
        "  ?   : Toggle this help menu",
        "  Esc : Close help menu",
        "  q   : Quit",
        "",
        "Press Esc or ? to close",
    ]

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
    global _last_render_params, _last_image_data

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
    draw_interface(cols, rows)

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
        }

        if current_params != _last_render_params or _last_image_data is None:
            image_data = render_mesh(
                mesh,
                inner_width_px,
                inner_height_px,
                view_axis=_view_axis,
                wireframe_thickness=_wireframe_thickness,
                up_vector_override=_up_vector_override,
            )
            _last_image_data = image_data
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
    - ?: Toggle help
    - Resize events
    """
    global _resize_pending
    global _show_help
    global _up_vector_cycle_index
    global _up_vector_override
    global _view_axis
    global _wireframe_thickness

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
                        needs_rerender = True
                    elif char == "X":
                        _view_axis = "-x"
                        needs_rerender = True
                    elif char == "y":
                        _view_axis = "+y"
                        needs_rerender = True
                    elif char == "Y":
                        _view_axis = "-y"
                        needs_rerender = True
                    elif char == "z":
                        _view_axis = "+z"
                        needs_rerender = True
                    elif char == "Z":
                        _view_axis = "-z"
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
