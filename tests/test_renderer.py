"""Tests for renderer module."""

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
        width, height = 400, 300
        image_data, camera_pos = render_mesh(cube, width, height)

        # Verify camera position is returned
        assert len(camera_pos) == 3

        # Verify we got raw RGBA data
        assert len(image_data) == width * height * 4

        # Verify image can be loaded as RGBA
        img = Image.frombytes("RGBA", (width, height), image_data)
        assert img.size == (width, height)

    def test_renders_sphere(self) -> None:
        """Test rendering a sphere mesh."""
        sphere = trimesh.creation.icosphere(subdivisions=2)

        width, height = 300, 300
        image_data, _ = render_mesh(sphere, width, height)

        # Verify raw RGBA format
        assert len(image_data) == width * height * 4
        img = Image.frombytes("RGBA", (width, height), image_data)
        assert img.size == (width, height)

    def test_renders_with_different_dimensions(self) -> None:
        """Test rendering with various dimensions."""
        mesh = trimesh.creation.box()

        # Test different aspect ratios
        for width, height in [(100, 100), (800, 600), (1920, 1080), (200, 400)]:
            image_data, _ = render_mesh(mesh, width, height)
            assert len(image_data) == width * height * 4
            img = Image.frombytes("RGBA", (width, height), image_data)
            assert img.size == (width, height)

    def test_renders_orthographic(self) -> None:
        """Test rendering with orthographic camera."""
        mesh = trimesh.creation.box()
        width, height = 400, 300
        image_data, _ = render_mesh(mesh, width, height, camera_type="orthographic")
        assert len(image_data) == width * height * 4
        img = Image.frombytes("RGBA", (width, height), image_data)
        assert img.size == (width, height)

    def test_renders_perspective(self) -> None:
        """Test rendering with perspective camera."""
        mesh = trimesh.creation.box()
        width, height = 400, 300
        image_data, _ = render_mesh(mesh, width, height, camera_type="perspective")
        assert len(image_data) == width * height * 4
        img = Image.frombytes("RGBA", (width, height), image_data)
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

        width, height = 500, 500
        image_data, _ = render_mesh(torus, width, height)

        # Verify raw RGBA format
        assert len(image_data) == width * height * 4
        img = Image.frombytes("RGBA", (width, height), image_data)
        assert img.size == (width, height)

    def test_rendered_image_not_empty(self) -> None:
        """Test that rendered image contains actual content."""
        mesh = trimesh.creation.box()

        width, height = 200, 200
        image_data, _ = render_mesh(mesh, width, height)
        img = Image.frombytes("RGBA", (width, height), image_data)

        # Convert to numpy array
        img_array = np.array(img)

        # Check that not all pixels are the same (image has content)
        # The image should have variations due to lighting and the mesh
        assert img_array.std() > 0

    def test_renders_small_dimensions(self) -> None:
        """Test rendering with very small dimensions."""
        mesh = trimesh.creation.box()

        # Should work even with small dimensions
        width, height = 10, 10
        image_data, _ = render_mesh(mesh, width, height)
        assert len(image_data) == width * height * 4
        img = Image.frombytes("RGBA", (width, height), image_data)
        assert img.size == (width, height)

    def test_renders_large_dimensions(self) -> None:
        """Test rendering with large dimensions."""
        mesh = trimesh.creation.box()

        # Should work with larger dimensions
        width, height = 2000, 1500
        image_data, _ = render_mesh(mesh, width, height)
        assert len(image_data) == width * height * 4
        img = Image.frombytes("RGBA", (width, height), image_data)
        assert img.size == (width, height)
