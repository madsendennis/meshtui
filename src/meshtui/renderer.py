"""Mesh rendering using pyrender."""

import io

import numpy as np
import pyrender
import trimesh
from PIL import Image


def render_mesh(mesh: trimesh.Trimesh, width: int, height: int) -> bytes:
    """Render a mesh to a PNG image.

    Creates a scene with the mesh, camera, and lighting, then renders it
    to a PNG image suitable for display in the terminal.

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

    # Create pyrender mesh from trimesh
    mesh_pr = pyrender.Mesh.from_trimesh(mesh)

    # Create scene
    scene = pyrender.Scene(ambient_light=[0.3, 0.3, 0.3])
    scene.add(mesh_pr)

    # Add directional light
    light = pyrender.DirectionalLight(color=[1.0, 1.0, 1.0], intensity=3.0)
    scene.add(light, pose=_get_light_pose())

    # Set up camera
    # Position camera to view the entire mesh
    camera = pyrender.PerspectiveCamera(yfov=np.pi / 3.0, aspectRatio=width / height)
    camera_pose = _calculate_camera_pose(mesh)
    scene.add(camera, pose=camera_pose)

    # Render with offscreen renderer
    renderer = pyrender.OffscreenRenderer(width, height)
    try:
        color, _ = renderer.render(scene)
    finally:
        renderer.delete()

    # Convert to PNG bytes
    image = Image.fromarray(color)
    img_bytes = io.BytesIO()
    image.save(img_bytes, format="PNG")
    return img_bytes.getvalue()


def _calculate_camera_pose(mesh: trimesh.Trimesh) -> np.ndarray:
    """Calculate camera pose to view the entire mesh.

    Positions camera at an angle to show the mesh clearly.

    Args:
        mesh: The mesh to view

    Returns:
        4x4 camera pose matrix
    """
    # Position camera at distance to see entire mesh
    # Using a fixed distance that works well for normalized meshes
    distance = 2.5

    # Position camera at 45 degree angle for better 3D visualization
    angle = np.pi / 4  # 45 degrees
    camera_pos = np.array([np.sin(angle) * distance, distance * 0.5, np.cos(angle) * distance])

    # Look at the center of the mesh
    target = np.array([0, 0, 0])
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
