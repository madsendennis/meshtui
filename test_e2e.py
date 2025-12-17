#!/usr/bin/env python3
"""End-to-end integration test."""

import os
import sys
from pathlib import Path
from unittest.mock import patch

import trimesh

# Add src to path
sys.path.insert(0, "src")

from meshtui.main import main
from meshtui.mesh_loader import load_mesh
from meshtui.renderer import render_mesh


def test_full_pipeline() -> int:
    """Test the complete pipeline with actual mesh file."""
    # Create a test mesh file
    print("Creating test mesh...")
    mesh = trimesh.creation.box(extents=[2, 2, 2])
    mesh.export("test_e2e_cube.ply")

    try:
        # Test 1: Load mesh
        print("Testing mesh loading...")
        loaded_mesh = load_mesh(Path("test_e2e_cube.ply"))
        assert loaded_mesh.vertices.shape[0] > 0, "Mesh has vertices"
        assert loaded_mesh.faces.shape[0] > 0, "Mesh has faces"
        print(
            f"  ✓ Loaded mesh with {len(loaded_mesh.vertices)} vertices, "
            f"{len(loaded_mesh.faces)} faces"
        )

        # Test 2: Render mesh
        print("Testing rendering...")
        png_data, camera_pos = render_mesh(loaded_mesh, width=800, height=600)
        assert len(png_data) > 0, "PNG data generated"
        assert png_data.startswith(b"\x89PNG"), "Valid PNG header"
        assert len(camera_pos) == 3, "Camera position returned"
        print(f"  ✓ Rendered mesh to PNG ({len(png_data)} bytes)")

        # Test 3: Full CLI pipeline (mocked)
        print("Testing CLI integration...")
        with (
            patch("meshtui.main.get_terminal_size") as mock_size,
            patch("meshtui.main.display_image") as mock_display,
        ):
            mock_size.return_value = (800, 600, 10, 20)

            with patch.object(sys, "argv", ["meshtui", "test_e2e_cube.ply"]):
                result = main()

                assert result == 0, "CLI succeeded"
                mock_display.assert_called_once()
                print("  ✓ CLI integration successful")

        print("\n✅ All end-to-end tests passed!")
        return 0

    finally:
        # Cleanup
        if os.path.exists("test_e2e_cube.ply"):
            os.remove("test_e2e_cube.ply")


if __name__ == "__main__":
    sys.exit(test_full_pipeline())
