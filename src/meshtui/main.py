"""Main entry point for meshtui CLI."""

import sys
from pathlib import Path

import typer

from meshtui.kitty_protocol import get_terminal_size
from meshtui.mesh_loader import load_meshes
from meshtui.screenshot import render_and_save_screenshot
from meshtui.tui import TUI


def parse_dimensions(dimension_str: str) -> tuple[int, int]:
    """Parse dimension string like '500x500' into (width, height).

    Args:
        dimension_str: String in format 'WIDTHxHEIGHT'

    Returns:
        Tuple of (width, height) as integers

    Raises:
        ValueError: If format is invalid or dimensions are non-positive
    """
    try:
        parts = dimension_str.lower().split("x")
        if len(parts) != 2:
            raise ValueError("Must be in format WIDTHxHEIGHT (e.g., 500x500)")
        width = int(parts[0])
        height = int(parts[1])
        if width <= 0 or height <= 0:
            raise ValueError("Dimensions must be positive integers")
        return width, height
    except ValueError as e:
        if "invalid literal" in str(e):
            raise ValueError(f"Invalid dimensions: {dimension_str}. Must be integers.") from e
        raise


def main(
    mesh_files: list[Path] = typer.Argument(  # noqa: B008
        ...,
        help="Path to mesh file(s) or directory to display (supports .ply, .stl, .obj, .drc, .glb)",
    ),
    screenshot: str = typer.Option(  # noqa: B008
        "",
        "--screenshot",
        help="Save screenshot without opening TUI. Format: WIDTHxHEIGHT (e.g., 500x500)",
    ),
) -> int:
    """Terminal-based 3D mesh viewer using Kitty graphics protocol.

    Load one or more 3D mesh files and interactively view them in your terminal with
    orbital camera controls, wireframe rendering, and lighting adjustments.

    Examples:
        meshtui model.ply
        meshtui part1.stl part2.stl
        meshtui ./models_directory/
        meshtui model.ply --screenshot 500x500
    """
    try:
        # Load meshes
        print(f"Loading meshes from: {', '.join(str(p) for p in mesh_files)}")
        try:
            meshes = load_meshes(mesh_files)
        except ValueError as e:
            print(f"Error: {e}", file=sys.stderr)
            return 1

        # Screenshot mode - no TUI, no Kitty protocol needed
        if screenshot:
            try:
                width, height = parse_dimensions(screenshot)
            except ValueError as e:
                print(f"Error: {e}", file=sys.stderr)
                return 1

            print(f"Rendering {width}x{height} screenshot...")

            try:
                output_path = render_and_save_screenshot(meshes, width, height)
                print(f"Screenshot saved: {output_path}")
                return 0
            except Exception as e:
                print(f"Error rendering screenshot: {e}", file=sys.stderr)
                return 1

        # Interactive TUI mode - requires Kitty protocol
        try:
            get_terminal_size()
        except RuntimeError as e:
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
