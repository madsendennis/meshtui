"""Screenshot utilities for saving rendered images."""

from datetime import datetime
from pathlib import Path

import numpy as np
import trimesh
from PIL import Image

from meshtui import config
from meshtui.camera_setup import (
    calculate_camera_distance,
    calculate_camera_parameters,
    get_camera_position_and_up,
)
from meshtui.renderer import render_mesh


def render_and_save_screenshot(
    meshes: list[trimesh.Trimesh], width: int, height: int, output_dir: Path | None = None
) -> Path:
    """Render meshes and save as PNG screenshot with default camera settings.

    Args:
        meshes: List of trimesh objects to render
        width: Image width in pixels
        height: Image height in pixels
        output_dir: Optional directory to save screenshot. If None, uses current working directory.

    Returns:
        Path to the saved screenshot file

    Raises:
        ValueError: If rendering fails
        OSError: If file cannot be written
    """
    # Load config once
    view_config = config.get_view_config()
    camera_config = config.get_camera_config()
    wireframe_config = config.get_wireframe_config()
    lighting_config = config.get_lighting_config()

    # Calculate camera parameters using shared logic
    view_axis = view_config["default_axis"]
    default_up_vector = tuple(view_config["up_vectors"][0])

    center, max_extent = calculate_camera_parameters(meshes, camera_config["distance_padding"])

    distance = calculate_camera_distance(
        max_extent,
        camera_config["distance_padding"],
        camera_config["fov_degrees"],
        camera_config["type"],
    )

    camera_pos, up_vector = get_camera_position_and_up(
        center, distance, view_axis, default_up_vector
    )

    # Render with swapped dimensions since we'll rotate 90 degrees
    # This ensures the final output has the correct dimensions after rotation
    image_data, _ = render_mesh(
        meshes,
        height,  # Swap width and height for rendering
        width,
        view_axis=view_axis,
        wireframe_thickness=wireframe_config["default_thickness"],
        up_vector_override=up_vector,
        orbital_eye=camera_pos,
        orbital_target=tuple(center),
        camera_type=camera_config["type"],
        light_intensity=lighting_config["key_light_intensity"],
    )

    # Save screenshot with rotation applied
    return save_screenshot(image_data, width, height, output_dir, needs_rotation=True)


def save_screenshot(
    image_data: bytes,
    width: int,
    height: int,
    output_dir: Path | None = None,
    needs_rotation: bool = False,
) -> Path:
    """Save raw RGBA image data as a PNG file with timestamp.

    Args:
        image_data: Raw RGBA image data as bytes from canvas
        width: Desired image width in pixels (final output width)
        height: Desired image height in pixels (final output height)
        output_dir: Optional directory to save screenshot. If None, uses current working directory.
        needs_rotation: If True, image will be rotated 90 degrees clockwise (for CLI screenshots)

    Returns:
        Path to the saved screenshot file

    Raises:
        ValueError: If image_data size doesn't match width*height*4
        OSError: If file cannot be written
    """
    # Validate image data size
    expected_size = width * height * 4  # RGBA = 4 bytes per pixel
    if len(image_data) != expected_size:
        raise ValueError(
            f"Image data size mismatch: expected {expected_size} bytes, got {len(image_data)}"
        )

    # Generate timestamp filename
    timestamp = datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
    filename = f"screenshot-{timestamp}.png"

    # Determine output path
    if output_dir is None:
        output_dir = Path.cwd()
    output_path = output_dir / filename

    if needs_rotation:
        # CLI screenshots: rendered with swapped dimensions, needs 90-degree rotation
        image_array = np.frombuffer(image_data, dtype=np.uint8).reshape(width, height, 4)
        img = Image.fromarray(image_array, mode="RGBA")
        img = img.transpose(Image.ROTATE_90)
    else:
        # TUI screenshots: already in correct orientation
        image_array = np.frombuffer(image_data, dtype=np.uint8).reshape(height, width, 4)
        img = Image.fromarray(image_array, mode="RGBA")

    # Save as PNG
    img.save(output_path, "PNG")

    return output_path
