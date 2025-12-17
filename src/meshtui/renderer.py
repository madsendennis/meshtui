"""Mesh rendering using pyrender."""

import io

import numpy as np
import pyrender
import trimesh
from PIL import Image

from meshtui.kitty_protocol import detect_terminal_background


def render_mesh(mesh: trimesh.Trimesh, width: int, height: int) -> bytes:
    """Render a mesh to a PNG image.

    Creates a scene with the mesh, camera, and lighting, then renders it
    to a PNG image suitable for display in the terminal. Uses transparent
    background to match terminal and adjusts mesh color for contrast.

    Args:
        mesh: The trimesh object to render
        width: Image width in pixels
        height: Image height in pixels

    Returns:
        PNG image data as bytes

    Raises:
        ValueError: If width or height are invalid
    """
    if width <= 0 or height <= 0:
        raise ValueError(f"Invalid dimensions: {width}x{height}")

    # Detect terminal background to choose appropriate mesh color
    is_light_bg = detect_terminal_background()

    # Create a copy of the mesh to modify its colors
    mesh = mesh.copy()

    # Set mesh color based on background
    # For light background: use dark gray (not black)
    # For dark background: use light gray (not white)
    if is_light_bg:
        # Dark gray for light backgrounds
        base_color = np.array([0.3, 0.3, 0.35, 1.0])  # Slightly bluish dark gray
    else:
        # Light gray for dark backgrounds
        base_color = np.array([0.75, 0.75, 0.8, 1.0])  # Slightly bluish light gray

    # Apply color to all vertices
    vertex_colors = np.tile(base_color, (len(mesh.vertices), 1))
    mesh.visual.vertex_colors = vertex_colors

    # Create pyrender mesh from trimesh
    mesh_pr = pyrender.Mesh.from_trimesh(mesh, smooth=True)

    # Create scene with no background (transparent)
    scene = pyrender.Scene(ambient_light=[0.4, 0.4, 0.4], bg_color=[0, 0, 0, 0])
    scene.add(mesh_pr)

    # Add directional light
    light = pyrender.DirectionalLight(color=[1.0, 1.0, 1.0], intensity=3.0)
    scene.add(light, pose=_get_light_pose())

    # Set up camera
    # Position camera to view the entire mesh, optimally filling the viewport
    aspect_ratio = width / height
    camera = pyrender.PerspectiveCamera(yfov=np.pi / 3.0, aspectRatio=aspect_ratio)
    camera_pose = _calculate_camera_pose(mesh, aspect_ratio)
    scene.add(camera, pose=camera_pose)

    # Render with offscreen renderer with alpha channel
    flags = pyrender.RenderFlags.RGBA
    renderer = pyrender.OffscreenRenderer(width, height)
    try:
        color, _ = renderer.render(scene, flags=flags)
    finally:
        renderer.delete()

    # Convert to PNG bytes with alpha channel
    image = Image.fromarray(color, mode="RGBA")
    img_bytes = io.BytesIO()
    image.save(img_bytes, format="PNG")
    return img_bytes.getvalue()


def _calculate_camera_pose(mesh: trimesh.Trimesh, aspect_ratio: float) -> np.ndarray:
    """Calculate camera pose to view the entire mesh optimally.

    Positions camera along the Z-axis looking at the mesh, with distance
    calculated to fit the entire mesh with padding.

    Args:
        mesh: The mesh to view
        aspect_ratio: Width/height ratio of the viewport

    Returns:
        4x4 camera pose matrix
    """
    # Get mesh bounding box (already centered at origin from mesh_loader)
    bounds = mesh.bounds
    mesh_size = np.max(bounds[1] - bounds[0])

    # Calculate optimal distance to fit mesh in view with generous padding
    # Field of view is 60 degrees (π/3), so we need distance = size / (2 * tan(fov/2))
    fov = np.pi / 3.0
    # Use 1.5x padding to ensure full mesh visibility with margin
    vertical_distance = (mesh_size * 1.5) / (2 * np.tan(fov / 2))

    # Account for aspect ratio
    if aspect_ratio > 1.0:
        # Wider screen - vertical extent is limiting factor
        distance = vertical_distance
    else:
        # Taller screen - need to check horizontal fit
        horizontal_fov = 2 * np.arctan(np.tan(fov / 2) * aspect_ratio)
        horizontal_distance = (mesh_size * 1.5) / (2 * np.tan(horizontal_fov / 2))
        distance = max(vertical_distance, horizontal_distance)

    # Position camera along the +Z axis (one of the main axes)
    # Mesh is centered at origin by mesh_loader, so target is [0,0,0]
    target = np.array([0.0, 0.0, 0.0])
    camera_pos = np.array([0.0, 0.0, distance])

    # Up vector
    up = np.array([0, 1, 0])

    # Create view matrix
    z = camera_pos - target
    z = z / np.linalg.norm(z)

    x = np.cross(up, z)
    x = x / np.linalg.norm(x)

    y = np.cross(z, x)

    # Build pose matrix (inverse of view matrix)
    pose = np.eye(4)
    pose[:3, 0] = x
    pose[:3, 1] = y
    pose[:3, 2] = z
    pose[:3, 3] = camera_pos

    return pose


def _get_light_pose() -> np.ndarray:
    """Get pose for directional light.

    Returns:
        4x4 light pose matrix
    """
    # Position light from upper right
    pose = np.eye(4)
    pose[:3, 3] = [1, 2, 1]
    return pose
