"""Main entry point for meshtui CLI."""

import sys
from pathlib import Path

import typer

from meshtui.kitty_protocol import get_terminal_size
from meshtui.mesh_loader import load_mesh
from meshtui.tui import TUI


def main(
    mesh_file: Path = typer.Argument(  # noqa: B008
        ...,
        help="Path to the mesh file to display (supports .ply, .stl, .obj, .drc, .glb)",
    ),
) -> int:
    """Terminal-based 3D mesh viewer using Kitty graphics protocol.

    Load a 3D mesh file and interactively view it in your terminal with
    orbital camera controls, wireframe rendering, and lighting adjustments.

    Examples:
        meshtui model.ply
        meshtui path/to/mesh.stl
    """
    mesh_path = mesh_file

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
        except FileNotFoundError:
            print(f"Error: File not found: {mesh_path}", file=sys.stderr)
            return 1
        except ValueError as e:
            print(f"Error: {e}", file=sys.stderr)
            return 1

        # Initialize and run TUI
        tui = TUI(mesh)
        tui.run()

        return 0

    except KeyboardInterrupt:
        return 0
    except Exception as e:
        print(f"Unexpected error: {e}", file=sys.stderr)
        return 1


app = typer.Typer(
    name="meshtui",
    help="Terminal-based 3D mesh viewer using Kitty graphics protocol",
)
app.command()(main)


if __name__ == "__main__":
    app()
