"""Main entry point for meshtui CLI."""

import sys
from pathlib import Path

from meshtui.kitty_protocol import display_image, get_terminal_size
from meshtui.mesh_loader import load_mesh
from meshtui.renderer import render_mesh


def main() -> int:
    """Main entry point for meshtui CLI.

    Loads a mesh file, renders it, and displays it in the terminal.

    Returns:
        Exit code (0 for success, non-zero for error)
    """
    # Parse arguments
    if len(sys.argv) < 2:
        print("Usage: meshtui <mesh_file>", file=sys.stderr)
        print("\nSupported formats: .ply, .stl", file=sys.stderr)
        return 1

    mesh_path = Path(sys.argv[1])

    try:
        # Check terminal compatibility
        try:
            width_px, height_px, cell_w, cell_h = get_terminal_size()
        except RuntimeError as e:
            print(f"Error: {e}", file=sys.stderr)
            print("\nThis application requires Kitty terminal.", file=sys.stderr)
            return 1

        # Load mesh
        print(f"Loading mesh: {mesh_path}")
        try:
            mesh = load_mesh(mesh_path)
        except FileNotFoundError:
            print(f"Error: File not found: {mesh_path}", file=sys.stderr)
            return 1
        except ValueError as e:
            print(f"Error: {e}", file=sys.stderr)
            return 1

        # Render mesh
        print(f"Rendering {len(mesh.vertices)} vertices, {len(mesh.faces)} faces...")
        try:
            image_data = render_mesh(mesh, width_px, height_px)
        except Exception as e:
            print(f"Error rendering mesh: {e}", file=sys.stderr)
            return 1

        # Display in terminal
        try:
            display_image(image_data, width_px, height_px)
        except RuntimeError as e:
            print(f"Error displaying image: {e}", file=sys.stderr)
            return 1

        return 0

    except KeyboardInterrupt:
        print("\nInterrupted by user", file=sys.stderr)
        return 130
    except Exception as e:
        print(f"Unexpected error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
