"""Tests for mesh_loader module."""

from pathlib import Path
from unittest.mock import patch

import numpy as np
import pytest
import trimesh

from meshtui.mesh_loader import load_mesh


class TestLoadMesh:
    """Tests for mesh loading functionality."""

    def test_loads_simple_mesh(self) -> None:
        """Test loading a simple cube mesh."""
        # Create a simple cube mesh in memory
        cube = trimesh.creation.box(extents=[2, 2, 2])

        with (
            patch("trimesh.load", return_value=cube),
            patch.object(Path, "exists", return_value=True),
        ):
            mesh = load_mesh(Path("fake.ply"))

            assert isinstance(mesh, trimesh.Trimesh)
            # Mesh should be centered (centroid at origin)
            assert np.allclose(mesh.centroid, [0, 0, 0], atol=1e-10)
            # Mesh should be normalized (largest dimension is 1)
            bounds = mesh.bounds
            max_dim = np.max(bounds[1] - bounds[0])
            assert np.isclose(max_dim, 1.0, atol=1e-10)

    def test_file_not_found(self) -> None:
        """Test error when file doesn't exist."""
        with (
            patch.object(Path, "exists", return_value=False),
            pytest.raises(FileNotFoundError, match="File not found"),
        ):
            load_mesh(Path("nonexistent.ply"))

    def test_unsupported_format(self) -> None:
        """Test error for unsupported file formats."""
        with (
            patch.object(Path, "exists", return_value=True),
            pytest.raises(ValueError, match="Unsupported file format"),
        ):
            load_mesh(Path("model.obj"))

    def test_supports_ply_format(self) -> None:
        """Test that .ply files are supported."""
        cube = trimesh.creation.box()

        with (
            patch("trimesh.load", return_value=cube),
            patch.object(Path, "exists", return_value=True),
        ):
            mesh = load_mesh(Path("model.ply"))
            assert isinstance(mesh, trimesh.Trimesh)

    def test_supports_stl_format(self) -> None:
        """Test that .stl files are supported."""
        cube = trimesh.creation.box()

        with (
            patch("trimesh.load", return_value=cube),
            patch.object(Path, "exists", return_value=True),
        ):
            mesh = load_mesh(Path("model.stl"))
            assert isinstance(mesh, trimesh.Trimesh)

    def test_case_insensitive_extension(self) -> None:
        """Test that file extensions are case-insensitive."""
        cube = trimesh.creation.box()

        with (
            patch("trimesh.load", return_value=cube),
            patch.object(Path, "exists", return_value=True),
        ):
            # Should work with uppercase extensions
            mesh = load_mesh(Path("model.PLY"))
            assert isinstance(mesh, trimesh.Trimesh)

    def test_handles_scene_object(self) -> None:
        """Test handling when trimesh.load returns a Scene instead of Mesh."""
        # Create a scene with a mesh
        cube = trimesh.creation.box()
        scene = trimesh.Scene()
        scene.add_geometry(cube, node_name="cube")

        with (
            patch("trimesh.load", return_value=scene),
            patch.object(Path, "exists", return_value=True),
        ):
            mesh = load_mesh(Path("model.ply"))
            assert isinstance(mesh, trimesh.Trimesh)

    def test_error_on_empty_scene(self) -> None:
        """Test error when scene has no geometry."""
        scene = trimesh.Scene()

        with (
            patch("trimesh.load", return_value=scene),
            patch.object(Path, "exists", return_value=True),
            pytest.raises(ValueError, match="No geometry found"),
        ):
            load_mesh(Path("empty.ply"))

    def test_error_on_invalid_mesh_data(self) -> None:
        """Test error when loaded object is not a mesh."""
        with (
            patch("trimesh.load", return_value="not a mesh"),
            patch.object(Path, "exists", return_value=True),
            pytest.raises(ValueError, match="not a valid mesh"),
        ):
            load_mesh(Path("invalid.ply"))

    def test_error_on_corrupt_file(self) -> None:
        """Test error handling for corrupt files."""
        with (
            patch("trimesh.load", side_effect=Exception("Corrupt file")),
            patch.object(Path, "exists", return_value=True),
            pytest.raises(ValueError, match="Failed to load mesh"),
        ):
            load_mesh(Path("corrupt.ply"))

    def test_normalizes_large_mesh(self) -> None:
        """Test that large meshes are normalized to unit scale."""
        # Create a large cube (100x100x100)
        large_cube = trimesh.creation.box(extents=[100, 100, 100])

        with (
            patch("trimesh.load", return_value=large_cube),
            patch.object(Path, "exists", return_value=True),
        ):
            mesh = load_mesh(Path("large.ply"))

            # Should be normalized to fit in 1x1x1 box
            bounds = mesh.bounds
            max_dim = np.max(bounds[1] - bounds[0])
            assert np.isclose(max_dim, 1.0, atol=1e-10)

    def test_normalizes_small_mesh(self) -> None:
        """Test that small meshes are normalized to unit scale."""
        # Create a small cube (0.01x0.01x0.01)
        small_cube = trimesh.creation.box(extents=[0.01, 0.01, 0.01])

        with (
            patch("trimesh.load", return_value=small_cube),
            patch.object(Path, "exists", return_value=True),
        ):
            mesh = load_mesh(Path("small.ply"))

            # Should be normalized to fit in 1x1x1 box
            bounds = mesh.bounds
            max_dim = np.max(bounds[1] - bounds[0])
            assert np.isclose(max_dim, 1.0, atol=1e-10)
