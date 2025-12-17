"""Main entry point for meshtui CLI."""

import signal
import sys
import termios
import tty
from pathlib import Path
from typing import Any

import trimesh

from meshtui.kitty_protocol import display_image, get_terminal_size
from meshtui.mesh_loader import load_mesh
from meshtui.renderer import render_mesh

# Global state for handling terminal resize
_current_mesh: trimesh.Trimesh | None = None
_resize_pending = False


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
    footer_text = " q: Quit | Esc: Exit | Ctrl+C: Exit"
    # Pad footer with spaces to clear line
    padding = " " * max(0, cols - len(footer_text))
    print(f"\033[{rows};1H{footer_text}{padding}", end="")

    # Flush
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
        image_data = render_mesh(mesh, inner_width_px, inner_height_px)
        # Move cursor to inside top-left (row 2, col 2)
        print("\033[2;2H", end="", flush=True)
        display_image(image_data, inner_width_px, inner_height_px, cols=inner_cols, rows=inner_rows)
    except Exception as e:
        if raise_errors:
            raise
        print(f"Error rendering/displaying: {e}", file=sys.stderr)


def wait_for_exit() -> None:
    """Wait for user to press 'q', Esc, or Ctrl+C to exit.

    Also handles terminal resize events and rerenders the mesh.
    """
    global _resize_pending

    if not sys.stdin.isatty():
        return

    # Don't print instructions in TUI mode to avoid scrolling
    # print("\nPress 'q', Esc, or Ctrl+C to exit. Terminal will auto-resize.", end="", flush=True)

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
                    # Check for 'q', 'Q', Esc (ASCII 27), or Ctrl+C (ASCII 3)
                    if char in ("q", "Q", "\x1b", "\x03"):
                        break
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
