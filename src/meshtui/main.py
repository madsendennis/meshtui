"""Main entry point for meshtui CLI."""

import sys
from pathlib import Path

import typer

from meshtui.kitty_protocol import get_terminal_size
from meshtui.mesh_loader import load_meshes
from meshtui.tui import TUI


def main(
    mesh_files: list[Path] = typer.Argument(  # noqa: B008
        ...,
        help="Path to mesh file(s) or directory to display (supports .ply, .stl, .obj, .drc, .glb)",
    ),
) -> int:
    """Terminal-based 3D mesh viewer using Kitty graphics protocol.

    Load one or more 3D mesh files and interactively view them in your terminal with
    orbital camera controls, wireframe rendering, and lighting adjustments.

    Examples:
        meshtui model.ply
        meshtui part1.stl part2.stl
        meshtui ./models_directory/
    """
    try:
        # Check terminal compatibility
        try:
            get_terminal_size()
        except RuntimeError as e:
            print(f"Error: {e}", file=sys.stderr)
            return 1

        # Load meshes
        print(f"Loading meshes from: {', '.join(str(p) for p in mesh_files)}")
        try:
            meshes = load_meshes(mesh_files)
        except ValueError as e:
            print(f"Error: {e}", file=sys.stderr)
            return 1

        # Initialize and run TUI
        tui = TUI(meshes)
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
