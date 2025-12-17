"""Mesh rendering using pyrender."""

import contextlib
import io

import numpy as np
import pyrender
import trimesh
from OpenGL import GL
from PIL import Image

from meshtui.kitty_protocol import detect_terminal_background

# Cache for the renderer to avoid recreating context
_renderer: pyrender.OffscreenRenderer | None = None
_renderer_size: tuple[int, int] = (0, 0)


def _get_renderer(width: int, height: int) -> pyrender.OffscreenRenderer:
    """Get or create a cached renderer instance."""
    global _renderer, _renderer_size

    if _renderer is None or _renderer_size != (width, height):
        if _renderer is not None:
            _renderer.delete()
        _renderer = pyrender.OffscreenRenderer(width, height)
        _renderer_size = (width, height)

    return _renderer


def render_mesh(
    mesh: trimesh.Trimesh,
    width: int,
    height: int,
    view_axis: str = "+z",
    wireframe_thickness: float = 0.0,
    up_vector_override: tuple[float, float, float] | None = None,
) -> bytes:
    """Render a mesh to a PNG image.

    Creates a scene with the mesh, camera, and lighting, then renders it
    to a PNG image suitable for display in the terminal. Uses transparent
    background to match terminal and adjusts mesh color for contrast.

    Args:
        mesh: The trimesh object to render
        width: Image width in pixels
        height: Image height in pixels
        view_axis: Camera view axis ('+x', '-x', '+y', '-y', '+z', '-z')
        wireframe_thickness: Thickness of wireframe lines. 0.0 means disabled.

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
    if is_light_bg:
        # Dark gray for light backgrounds
        base_color = np.array([0.3, 0.3, 0.35, 1.0])
        wireframe_color = [0, 0, 0, 255]  # Black wireframe
    else:
        # Light gray for dark backgrounds
        base_color = np.array([0.75, 0.75, 0.8, 1.0])
        wireframe_color = [255, 255, 255, 255]  # White wireframe

    # Apply color to all vertices
    vertex_colors = np.tile(base_color, (len(mesh.vertices), 1))
    mesh.visual.vertex_colors = vertex_colors

    # Create pyrender mesh from trimesh
    mesh_pr = pyrender.Mesh.from_trimesh(mesh, smooth=True)

    # Create scene with no background (transparent)
    scene = pyrender.Scene(ambient_light=[0.4, 0.4, 0.4], bg_color=[0, 0, 0, 0])
    scene.add(mesh_pr)

    # Add wireframe if requested
    if wireframe_thickness > 0.0:
        # Get unique edges for wireframe
        edges = mesh.edges_unique
        lines = mesh.vertices[edges]

        # Create line segments
        # Flatten lines array for pyrender Primitive
        positions = lines.reshape(-1, 3)

        # Create primitive for lines
        wireframe = pyrender.Primitive(
            positions=positions,
            mode=1,  # GL_LINES
            color_0=wireframe_color,
        )

        # Add wireframe mesh to scene
        wireframe_mesh = pyrender.Mesh([wireframe])
        scene.add(wireframe_mesh)

    # Set up camera
    aspect_ratio = width / height
    camera = pyrender.PerspectiveCamera(yfov=np.pi / 3.0, aspectRatio=aspect_ratio)
    camera_node = scene.add(camera)
    camera_pose = _calculate_camera_pose(
        mesh,
        aspect_ratio,
        view_axis=view_axis,
        yfov=camera.yfov,
        up_vector_override=up_vector_override,
    )
    scene.set_pose(camera_node, camera_pose)

    # Add lighting
    # Directional light from camera position
    light = pyrender.DirectionalLight(color=[1.0, 1.0, 1.0], intensity=3.0)
    light_node = scene.add(light)
    scene.set_pose(light_node, camera_pose)

    # Add fill light from opposite direction
    fill_light_pose = camera_pose.copy()
    fill_light_pose[:3, 3] = -fill_light_pose[:3, 3]  # Invert position
    fill_light = pyrender.DirectionalLight(color=[1.0, 1.0, 1.0], intensity=1.5)
    fill_light_node = scene.add(fill_light)
    scene.set_pose(fill_light_node, fill_light_pose)

    # Render with offscreen renderer with alpha channel
    flags = pyrender.RenderFlags.RGBA
    renderer = _get_renderer(width, height)

    # Attempt to set line width if wireframe is enabled
    if wireframe_thickness > 0.0:
        with contextlib.suppress(Exception):
            # This requires the context to be active, which pyrender handles during render
            # But we can try to set it globally for the context.
            renderer._platform.make_current()
            GL.glLineWidth(wireframe_thickness)

    color, _ = renderer.render(scene, flags=flags)

    # Reset line width
    if wireframe_thickness > 0.0:
        with contextlib.suppress(Exception):
            GL.glLineWidth(1.0)

    # Convert to PNG bytes with alpha channel
    image = Image.fromarray(color, mode="RGBA")
    img_bytes = io.BytesIO()
    image.save(img_bytes, format="PNG")
    return img_bytes.getvalue()


def _calculate_camera_pose(
    mesh: trimesh.Trimesh,
    aspect_ratio: float,
    view_axis: str = "+z",
    yfov: float = np.pi / 3.0,
    up_vector_override: tuple[float, float, float] | None = None,
) -> np.ndarray:
    """Calculate camera pose to view the entire mesh optimally.

    Positions camera along the specified axis looking at the mesh, with distance
    calculated to fit the entire mesh with padding.

    Args:
        mesh: The mesh to view
        aspect_ratio: Width/height ratio of the viewport
        view_axis: Camera view axis ('+x', '-x', '+y', '-y', '+z', '-z')

    Returns:
        4x4 camera pose matrix
    """
    # World-space AABB center
    bounds = mesh.bounds
    aabb_min = bounds[0]
    aabb_max = bounds[1]
    center = (aabb_min + aabb_max) / 2.0

    # Auto-distance to fit: D = MaxExtent / tan(FOV/2)
    extents = aabb_max - aabb_min
    max_extent = float(np.max(extents))
    tan_half_fov = float(np.tan(yfov / 2.0))
    if not np.isfinite(tan_half_fov) or tan_half_fov <= 0.0:
        tan_half_fov = 1e-6
    distance = max_extent / tan_half_fov
    if not np.isfinite(distance) or distance <= 0.0:
        distance = 1.0

    axis = view_axis.lower()

    # Predefined view ups (used when no override is requested)
    if axis in {"+y", "-y"}:
        default_up = np.array([0.0, 0.0, -1.0]) if axis == "+y" else np.array([0.0, 0.0, 1.0])
    else:
        default_up = np.array([0.0, 1.0, 0.0])

    up = default_up if up_vector_override is None else np.array(up_vector_override, dtype=float)

    if axis == "+z":
        eye = np.array([center[0], center[1], center[2] + distance])
    elif axis == "-z":
        eye = np.array([center[0], center[1], center[2] - distance])
    elif axis == "+x":
        eye = np.array([center[0] + distance, center[1], center[2]])
    elif axis == "-x":
        eye = np.array([center[0] - distance, center[1], center[2]])
    elif axis == "+y":
        eye = np.array([center[0], center[1] + distance, center[2]])
    elif axis == "-y":
        eye = np.array([center[0], center[1] - distance, center[2]])
    else:
        eye = np.array([center[0], center[1], center[2] + distance])

    return _look_at_pose(eye=eye, target=center, up=up)


def _look_at_pose(eye: np.ndarray, target: np.ndarray, up: np.ndarray) -> np.ndarray:
    """Create a pyrender camera pose matrix from eye/target/up.

    Returns a camera-to-world transform where the camera looks towards target.
    Pyrender uses an OpenGL-style camera where -Z is forward.
    """
    eye = np.asarray(eye, dtype=float)
    target = np.asarray(target, dtype=float)
    up = np.asarray(up, dtype=float)

    z = eye - target
    z_norm = np.linalg.norm(z)
    z = np.array([0.0, 0.0, 1.0]) if z_norm == 0 or not np.isfinite(z_norm) else z / z_norm

    x = np.cross(up, z)
    x_norm = np.linalg.norm(x)
    if x_norm == 0 or not np.isfinite(x_norm):
        # Fallback: choose an alternate up if up is parallel to view direction
        fallback_up = np.array([0.0, 0.0, 1.0]) if abs(z[2]) < 0.9 else np.array([0.0, 1.0, 0.0])
        x = np.cross(fallback_up, z)
        x_norm = np.linalg.norm(x)
        if x_norm == 0 or not np.isfinite(x_norm):
            x = np.array([1.0, 0.0, 0.0])
            x_norm = 1.0
    x = x / x_norm

    y = np.cross(z, x)

    pose = np.eye(4)
    pose[:3, 0] = x
    pose[:3, 1] = y
    pose[:3, 2] = z
    pose[:3, 3] = eye
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
