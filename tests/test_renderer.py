"""Tests for renderer module."""

import io

import numpy as np
import pytest
import trimesh
from PIL import Image

from meshtui.renderer import render_mesh


class TestRenderMesh:
    """Tests for mesh rendering functionality."""

    def test_renders_simple_cube(self) -> None:
        """Test rendering a simple cube mesh."""
        # Create a simple cube
        cube = trimesh.creation.box(extents=[1, 1, 1])

        # Render at a reasonable size
        image_data = render_mesh(cube, 400, 300)

        # Verify we got PNG data
        assert image_data.startswith(b"\x89PNG")

        # Verify image can be loaded
        img = Image.open(io.BytesIO(image_data))
        assert img.format == "PNG"
        assert img.size == (400, 300)

    def test_renders_sphere(self) -> None:
        """Test rendering a sphere mesh."""
        sphere = trimesh.creation.icosphere(subdivisions=2)

        image_data = render_mesh(sphere, 300, 300)

        # Verify PNG format
        assert image_data.startswith(b"\x89PNG")
        img = Image.open(io.BytesIO(image_data))
        assert img.size == (300, 300)

    def test_renders_with_different_dimensions(self) -> None:
        """Test rendering with various dimensions."""
        mesh = trimesh.creation.box()

        # Test different aspect ratios
        for width, height in [(100, 100), (800, 600), (1920, 1080), (200, 400)]:
            image_data = render_mesh(mesh, width, height)
            img = Image.open(io.BytesIO(image_data))
            assert img.size == (width, height)

    def test_invalid_dimensions_zero_width(self) -> None:
        """Test error handling for zero width."""
        mesh = trimesh.creation.box()

        with pytest.raises(ValueError, match="Invalid dimensions"):
            render_mesh(mesh, 0, 100)

    def test_invalid_dimensions_zero_height(self) -> None:
        """Test error handling for zero height."""
        mesh = trimesh.creation.box()

        with pytest.raises(ValueError, match="Invalid dimensions"):
            render_mesh(mesh, 100, 0)

    def test_invalid_dimensions_negative(self) -> None:
        """Test error handling for negative dimensions."""
        mesh = trimesh.creation.box()

        with pytest.raises(ValueError, match="Invalid dimensions"):
            render_mesh(mesh, -100, 100)

    def test_renders_complex_mesh(self) -> None:
        """Test rendering a more complex mesh."""
        # Create a torus (more complex than cube)
        torus = trimesh.creation.torus(major_radius=0.5, minor_radius=0.2)

        image_data = render_mesh(torus, 500, 500)

        # Verify PNG format
        assert image_data.startswith(b"\x89PNG")
        img = Image.open(io.BytesIO(image_data))
        assert img.size == (500, 500)

    def test_rendered_image_not_empty(self) -> None:
        """Test that rendered image contains actual content."""
        mesh = trimesh.creation.box()

        image_data = render_mesh(mesh, 200, 200)
        img = Image.open(io.BytesIO(image_data))

        # Convert to numpy array
        img_array = np.array(img)

        # Check that not all pixels are the same (image has content)
        # The image should have variations due to lighting and the mesh
        assert img_array.std() > 0

    def test_renders_small_dimensions(self) -> None:
        """Test rendering with very small dimensions."""
        mesh = trimesh.creation.box()

        # Should work even with small dimensions
        image_data = render_mesh(mesh, 10, 10)
        img = Image.open(io.BytesIO(image_data))
        assert img.size == (10, 10)

    def test_renders_large_dimensions(self) -> None:
        """Test rendering with large dimensions."""
        mesh = trimesh.creation.box()

        # Should work with larger dimensions
        image_data = render_mesh(mesh, 2000, 1500)
        img = Image.open(io.BytesIO(image_data))
        assert img.size == (2000, 1500)
