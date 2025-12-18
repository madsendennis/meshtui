"""Mesh rendering using pyrender."""

import contextlib

import numpy as np
import pyrender
import trimesh
from OpenGL import GL

from meshtui import config
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
    orbital_eye: tuple[float, float, float] | None = None,
    orbital_target: tuple[float, float, float] | None = None,
) -> tuple[bytes, tuple[float, float, float]]:
    """Render a mesh to raw RGBA image data.

    Creates a scene with the mesh, camera, and lighting, then renders it
    to a raw RGBA buffer suitable for display in the terminal. Uses transparent
    background to match terminal and adjusts mesh color for contrast.

    Args:
        mesh: The trimesh object to render
        width: Image width in pixels
        height: Image height in pixels
        view_axis: Camera view axis ('+x', '-x', '+y', '-y', '+z', '-z')
        wireframe_thickness: Thickness of wireframe lines. 0.0 means disabled.
        up_vector_override: Optional up vector for camera orientation
        orbital_eye: Optional camera eye position for orbital mode
        orbital_target: Optional camera target position for orbital mode

    Returns:
        Tuple of (Raw RGBA image data as bytes, camera position as (x, y, z))

    Raises:
        ValueError: If width or height are invalid
    """
    if width <= 0 or height <= 0:
        raise ValueError(f"Invalid dimensions: {width}x{height}")

    # Load configuration
    wireframe_cfg = config.get_wireframe_config()
    scene_cfg = config.get_scene_config()
    material_cfg = config.get_material_config()
    lighting_cfg = config.get_lighting_config()

    # Detect terminal background to choose appropriate colors
    is_light_bg = detect_terminal_background()

    # Choose wireframe color and background based on terminal
    if is_light_bg:
        wireframe_color = wireframe_cfg["color_light_bg"]
        bg_color = scene_cfg["bg_color_light"]
    else:
        wireframe_color = wireframe_cfg["color_dark_bg"]
        bg_color = scene_cfg["bg_color_dark"]

    # Create a copy of the mesh
    mesh = mesh.copy()

    # Create PBR metallic-roughness material from config
    pbr_material = pyrender.MetallicRoughnessMaterial(
        baseColorFactor=material_cfg["base_color"],
        metallicFactor=material_cfg["metallic_factor"],
        roughnessFactor=material_cfg["roughness_factor"],
        alphaMode="OPAQUE",
        doubleSided=False,
    )

    # Create pyrender mesh with PBR material and smooth shading
    mesh_pr = pyrender.Mesh.from_trimesh(mesh, material=pbr_material, smooth=True)

    # Create scene with ambient light and background from config
    scene = pyrender.Scene(ambient_light=scene_cfg["ambient_light"], bg_color=bg_color)
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

    # Set up camera with FOV from config
    camera_cfg = config.get_camera_config()
    aspect_ratio = width / height
    fov_radians = np.radians(camera_cfg["fov_degrees"])
    camera = pyrender.PerspectiveCamera(yfov=fov_radians, aspectRatio=aspect_ratio)
    camera_node = scene.add(camera)

    # Use orbital camera if eye and target are provided
    if orbital_eye is not None and orbital_target is not None:
        # Use provided up vector or fallback
        if up_vector_override is None:
            up = np.array([0.0, 0.0, 1.0])
        else:
            up = np.array(up_vector_override, dtype=float)
        camera_pose = _look_at_pose(
            eye=np.array(orbital_eye), target=np.array(orbital_target), up=up
        )
    else:
        # Use axis-based camera positioning
        camera_pose = _calculate_camera_pose(
            mesh,
            aspect_ratio,
            view_axis=view_axis,
            yfov=camera.yfov,
            distance_padding=camera_cfg["distance_padding"],
            up_vector_override=up_vector_override,
        )
    scene.set_pose(camera_node, camera_pose)

    # Implement Raymond 3-point lighting system from config
    # All lights are positioned relative to camera pose for dynamic lighting

    # 1. Key Light (Headlamp): Main light straight from camera
    key_light = pyrender.DirectionalLight(
        color=[1.0, 1.0, 1.0], intensity=lighting_cfg["key_light_intensity"]
    )
    key_light_node = scene.add(key_light)
    scene.set_pose(key_light_node, camera_pose)

    # 2. Fill Light: offset from camera
    fill_light = pyrender.DirectionalLight(
        color=[1.0, 1.0, 1.0], intensity=lighting_cfg["fill_light_intensity"]
    )
    fill_light_node = scene.add(fill_light)
    fill_light_pose = _compute_offset_light_pose(
        camera_pose,
        azimuth_deg=lighting_cfg["fill_light_azimuth"],
        elevation_deg=lighting_cfg["fill_light_elevation"],
    )
    scene.set_pose(fill_light_node, fill_light_pose)

    # 3. Back/Rim Light: offset from camera for edge definition
    rim_light = pyrender.DirectionalLight(
        color=[1.0, 1.0, 1.0], intensity=lighting_cfg["rim_light_intensity"]
    )
    rim_light_node = scene.add(rim_light)
    rim_light_pose = _compute_offset_light_pose(
        camera_pose,
        azimuth_deg=lighting_cfg["rim_light_azimuth"],
        elevation_deg=lighting_cfg["rim_light_elevation"],
    )
    scene.set_pose(rim_light_node, rim_light_pose)

    # Render with offscreen renderer with alpha channel
    flags = pyrender.RenderFlags.RGBA

    # Enable shadows if configured (disabled by default for performance)
    perf_cfg = config.get_performance_config()
    if perf_cfg["enable_shadows"]:
        flags |= pyrender.RenderFlags.SHADOWS_DIRECTIONAL

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

    # Return raw RGBA bytes
    # color is a numpy array of shape (height, width, 4) with dtype uint8
    raw_bytes = color.tobytes()

    # Extract camera position from pose matrix
    camera_pos = tuple(camera_pose[:3, 3])

    return raw_bytes, camera_pos


def _calculate_camera_pose(
    mesh: trimesh.Trimesh,
    aspect_ratio: float,
    view_axis: str = "+z",
    yfov: float = np.pi / 3.0,
    distance_padding: float = 1.0,
    up_vector_override: tuple[float, float, float] | None = None,
) -> np.ndarray:
    """Calculate camera pose to view the entire mesh optimally.

    Positions camera along the specified axis looking at the mesh, with distance
    calculated to fit the entire mesh with padding.

    Args:
        mesh: The mesh to view
        aspect_ratio: Width/height ratio of the viewport
        view_axis: Camera view axis ('+x', '-x', '+y', '-y', '+z', '-z')
        yfov: Vertical field of view in radians
        distance_padding: Multiplier for calculated distance (larger = farther)
        up_vector_override: Optional up vector override

    Returns:
        4x4 camera pose matrix
    """
    # World-space AABB center
    bounds = mesh.bounds
    aabb_min = bounds[0]
    aabb_max = bounds[1]
    center = (aabb_min + aabb_max) / 2.0

    # Auto-distance to fit: D = (MaxExtent / tan(FOV/2)) * padding
    extents = aabb_max - aabb_min
    max_extent = float(np.max(extents))
    tan_half_fov = float(np.tan(yfov / 2.0))
    if not np.isfinite(tan_half_fov) or tan_half_fov <= 0.0:
        tan_half_fov = 1e-6
    distance = (max_extent / tan_half_fov) * distance_padding
    if not np.isfinite(distance) or distance <= 0.0:
        distance = 1.0

    axis = view_axis.lower()

    # Use the provided up vector (will be config default when not explicitly cycling)
    if up_vector_override is None:
        # Fallback to Y-up if no override provided (shouldn't happen in practice)
        up = np.array([0.0, 1.0, 0.0])
    else:
        up = np.array(up_vector_override, dtype=float)

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


def _compute_offset_light_pose(
    camera_pose: np.ndarray, azimuth_deg: float, elevation_deg: float
) -> np.ndarray:
    """Compute a light pose offset from the camera pose.

    Args:
        camera_pose: 4x4 camera pose matrix
        azimuth_deg: Azimuth angle in degrees (rotation around Y-axis)
        elevation_deg: Elevation angle in degrees (rotation around X-axis)

    Returns:
        4x4 light pose matrix
    """
    # Extract camera orientation and position
    cam_right = camera_pose[:3, 0]
    cam_up = camera_pose[:3, 1]
    cam_forward = camera_pose[:3, 2]
    cam_pos = camera_pose[:3, 3]

    # Convert angles to radians
    azimuth_rad = np.radians(azimuth_deg)
    elevation_rad = np.radians(elevation_deg)

    # Rotate around camera's up axis (azimuth)
    cos_az = np.cos(azimuth_rad)
    sin_az = np.sin(azimuth_rad)
    # Rotate the forward vector around up
    rotated_forward = cos_az * cam_forward + sin_az * cam_right

    # Rotate around the right axis (elevation)
    cos_el = np.cos(elevation_rad)
    sin_el = np.sin(elevation_rad)
    final_forward = cos_el * rotated_forward + sin_el * cam_up

    # Normalize
    final_forward = final_forward / np.linalg.norm(final_forward)

    # Recompute right and up vectors
    final_right = np.cross(cam_up, final_forward)
    final_right = final_right / np.linalg.norm(final_right)
    final_up = np.cross(final_forward, final_right)

    # Build light pose
    light_pose = np.eye(4)
    light_pose[:3, 0] = final_right
    light_pose[:3, 1] = final_up
    light_pose[:3, 2] = final_forward
    light_pose[:3, 3] = cam_pos

    return light_pose


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
