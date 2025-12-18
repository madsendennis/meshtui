"""Mesh rendering using pygfx with GPU acceleration."""

import numpy as np
import pygfx as gfx
import trimesh
import wgpu  # type: ignore
from rendercanvas.offscreen import RenderCanvas

from meshtui import config
from meshtui.kitty_protocol import detect_terminal_background

# Suppress wgpu warnings (like VK_EXT_physical_device_drm missing)
wgpu.logger.setLevel("ERROR")

# Cache for the renderer to avoid recreating context
_renderer: gfx.renderers.WgpuRenderer | None = None
_canvas: RenderCanvas | None = None
_canvas_size: tuple[int, int] = (0, 0)


def _get_renderer(width: int, height: int) -> tuple[gfx.renderers.WgpuRenderer, RenderCanvas]:
    """Get or create a cached renderer instance with offscreen canvas."""
    global _renderer, _canvas, _canvas_size

    if _renderer is None or _canvas_size != (width, height):
        if _canvas is not None:
            _canvas.close()

        # Create offscreen canvas with exact dimensions
        _canvas = RenderCanvas(size=(width, height), pixel_ratio=1)
        _renderer = gfx.renderers.WgpuRenderer(_canvas)
        _canvas_size = (width, height)

    return _renderer, _canvas


def render_mesh(
    mesh: trimesh.Trimesh,
    width: int,
    height: int,
    view_axis: str = "+z",
    wireframe_thickness: float = 0.0,
    up_vector_override: tuple[float, float, float] | None = None,
    orbital_eye: tuple[float, float, float] | None = None,
    orbital_target: tuple[float, float, float] | None = None,
    camera_type: str = "orthographic",
    light_intensity: float | None = None,
    ortho_zoom: float = 1.0,
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
        camera_type: Camera type ('perspective' or 'orthographic')
        light_intensity: Key light intensity multiplier (default: 3.5)
        ortho_zoom: Orthographic camera zoom factor (smaller = more zoomed in, default: 1.0)

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
    camera_cfg = config.get_camera_config()

    # Use config default if light intensity not provided
    if light_intensity is None:
        light_intensity = lighting_cfg["key_light_intensity"]

    # Detect terminal background to choose appropriate colors
    is_light_bg = detect_terminal_background()

    # Choose wireframe color and background based on terminal
    if is_light_bg:
        wireframe_color = wireframe_cfg["color_light_bg"]
        bg_color = scene_cfg["bg_color_light"]
    else:
        wireframe_color = wireframe_cfg["color_dark_bg"]
        bg_color = scene_cfg["bg_color_dark"]

    # Create scene
    scene = gfx.Scene()

    # Add background with transparency
    bg_color_normalized = tuple(c / 255.0 if i < 3 else c for i, c in enumerate(bg_color))
    background = gfx.Background.from_color(bg_color_normalized)
    scene.add(background)

    # Convert trimesh to pygfx geometry
    positions = mesh.vertices.astype(np.float32)
    indices = mesh.faces.astype(np.uint32)

    # Compute normals using trimesh (requires scipy for weighted vertex normals)
    if hasattr(mesh, "vertex_normals"):
        normals = mesh.vertex_normals.astype(np.float32)
    else:
        normals = np.zeros_like(positions)

    geometry = gfx.Geometry(
        positions=positions,
        indices=indices,
        normals=normals,
    )

    # Create material for solid mesh
    base_color = tuple(material_cfg["base_color"])
    material = gfx.MeshPhongMaterial(
        color=base_color,
        shininess=int((1.0 - material_cfg["roughness_factor"]) * 100),
    )

    # Create solid mesh object
    mesh_obj = gfx.Mesh(geometry, material)
    scene.add(mesh_obj)

    # Add wireframe overlay if requested
    if wireframe_thickness > 0.0:
        wireframe_color_normalized = tuple(
            c / 255.0 if i < 3 else c for i, c in enumerate(wireframe_color)
        )
        wireframe_material = gfx.MeshBasicMaterial(
            color=wireframe_color_normalized,
            wireframe=True,
            wireframe_thickness=wireframe_thickness,
        )
        wireframe_obj = gfx.Mesh(geometry, wireframe_material)
        scene.add(wireframe_obj)

    # Calculate camera position and setup
    bounds = mesh.bounds
    center = (bounds[0] + bounds[1]) / 2.0
    extents = bounds[1] - bounds[0]
    max_extent = float(np.max(extents))

    # Use orbital camera if eye and target are provided
    if orbital_eye is not None and orbital_target is not None:
        camera_pos = np.array(orbital_eye)
        target = np.array(orbital_target)
        up = np.array(up_vector_override if up_vector_override else [0.0, 0.0, 1.0])
    else:
        # Calculate camera position based on view axis
        distance_padding = camera_cfg["distance_padding"]
        distance = max_extent * distance_padding * 2.0  # Conservative distance

        up = np.array(up_vector_override if up_vector_override else [0.0, 1.0, 0.0])

        axis = view_axis.lower()
        if axis == "+z":
            camera_pos = np.array([center[0], center[1], center[2] + distance])
        elif axis == "-z":
            camera_pos = np.array([center[0], center[1], center[2] - distance])
        elif axis == "+x":
            camera_pos = np.array([center[0] + distance, center[1], center[2]])
        elif axis == "-x":
            camera_pos = np.array([center[0] - distance, center[1], center[2]])
        elif axis == "+y":
            camera_pos = np.array([center[0], center[1] + distance, center[2]])
        elif axis == "-y":
            camera_pos = np.array([center[0], center[1] - distance, center[2]])
        else:
            camera_pos = np.array([center[0], center[1], center[2] + distance])

        target = center

    # Create camera
    aspect_ratio = width / height
    if camera_type == "orthographic":
        # For orthographic, use magnification to fit the mesh
        if aspect_ratio > 1.0:
            ymag = max_extent * camera_cfg["distance_padding"]
            xmag = ymag * aspect_ratio
        else:
            xmag = max_extent * camera_cfg["distance_padding"]
            ymag = xmag / aspect_ratio

        # Apply zoom factor (dividing magnification = more zoomed in)
        xmag /= ortho_zoom
        ymag /= ortho_zoom

        camera = gfx.OrthographicCamera(xmag * 2, ymag * 2)
    else:
        # Perspective camera
        fov = camera_cfg["fov_degrees"]
        # Set reasonable near/far clipping planes for perspective camera
        # Near plane should be small but not too small to avoid z-fighting
        # Far plane should be large enough to encompass the scene
        near_plane = max_extent * 0.001  # Very close to camera
        far_plane = max_extent * 100.0  # Far away
        camera = gfx.PerspectiveCamera(fov, aspect_ratio, depth_range=(near_plane, far_plane))

    # Position camera and orient it to look at target with proper up vector
    import pylinalg as la

    # Use mat_look_at(target, eye, up) to align -Z (forward) towards target
    view_matrix = la.mat_look_at(target, camera_pos, up)
    view_matrix[:3, 3] = camera_pos
    camera.local.matrix = view_matrix
    scene.add(camera)

    # Compute safe up vector for lights to avoid gimbal lock
    # If view direction is nearly parallel to up vector, use an alternate up vector
    view_dir = target - camera_pos
    view_dir_norm = view_dir / np.linalg.norm(view_dir)
    up_norm = up / np.linalg.norm(up)

    # Check if view direction and up vector are nearly parallel (dot product close to ±1)
    dot_product = abs(float(np.dot(view_dir_norm, up_norm)))
    if dot_product > 0.95:  # Nearly parallel
        # Choose an alternate up vector perpendicular to view direction
        # If current up is Y-up, use Z-up; if Z-up, use Y-up
        light_up = np.array([0.0, 0.0, 1.0]) if abs(up[1]) > 0.5 else np.array([0.0, 1.0, 0.0])
    else:
        light_up = up

    # Add lighting - always enabled so both solid mesh and wireframe can be seen together
    # Ambient light
    ambient = gfx.AmbientLight(intensity=scene_cfg["ambient_light"][0])
    scene.add(ambient)

    # Key light (main directional light) - controlled by user
    # Attached to camera to act as a headlight
    key_light = gfx.DirectionalLight(intensity=light_intensity)
    camera.add(key_light)

    # Fill light (offset directional light) - proportional to key light
    fill_intensity = light_intensity * (
        lighting_cfg["fill_light_intensity"] / lighting_cfg["key_light_intensity"]
    )
    fill_light = gfx.DirectionalLight(intensity=fill_intensity)
    fill_offset = _apply_angular_offset(
        camera_pos,
        target,
        lighting_cfg["fill_light_azimuth"],
        lighting_cfg["fill_light_elevation"],
    )
    fill_light_matrix = la.mat_look_at(target, fill_offset, light_up)
    fill_light_matrix[:3, 3] = fill_offset
    fill_light.local.matrix = fill_light_matrix
    scene.add(fill_light)
    # Rim light (back light for edge definition) - proportional to key light
    rim_intensity = light_intensity * (
        lighting_cfg["rim_light_intensity"] / lighting_cfg["key_light_intensity"]
    )
    rim_light = gfx.DirectionalLight(intensity=rim_intensity)
    rim_offset = _apply_angular_offset(
        camera_pos,
        target,
        lighting_cfg["rim_light_azimuth"],
        lighting_cfg["rim_light_elevation"],
    )
    rim_light_matrix = la.mat_look_at(target, rim_offset, light_up)
    rim_light_matrix[:3, 3] = rim_offset
    rim_light.local.matrix = rim_light_matrix
    rim_light_matrix = la.mat_look_at(rim_offset, target, light_up)
    rim_light.local.matrix = rim_light_matrix
    rim_light.local.position = tuple(rim_offset)
    scene.add(rim_light)

    # Get renderer and render
    renderer, canvas = _get_renderer(width, height)

    # Render the scene
    canvas.request_draw(lambda: renderer.render(scene, camera))

    # Get the raw RGBA data from canvas
    # The draw() method returns a memoryview that we can convert to bytes
    image_array = np.asarray(canvas.draw())

    # Ensure RGBA format (height, width, 4)
    if image_array.shape[-1] != 4:
        raise RuntimeError(f"Expected RGBA output, got shape {image_array.shape}")

    # Convert to bytes (row-major order)
    raw_bytes = image_array.tobytes()
    camera_pos_tuple = tuple(camera_pos)

    return raw_bytes, camera_pos_tuple


def _apply_angular_offset(
    camera_pos: np.ndarray,
    target: np.ndarray,
    azimuth_deg: float,
    elevation_deg: float,
) -> np.ndarray:
    """Apply angular offset to camera position for light positioning.

    Args:
        camera_pos: Camera position
        target: Look-at target
        azimuth_deg: Azimuth angle in degrees (horizontal rotation)
        elevation_deg: Elevation angle in degrees (vertical rotation)

    Returns:
        New position with angular offset applied
    """
    # Calculate view direction
    view_dir = camera_pos - target
    distance = float(np.linalg.norm(view_dir))

    # Convert to spherical coordinates
    xy_dist = np.sqrt(view_dir[0] ** 2 + view_dir[1] ** 2)
    theta = np.arctan2(view_dir[1], view_dir[0])  # azimuth
    phi = np.arctan2(view_dir[2], xy_dist)  # elevation

    # Apply offsets
    theta += np.radians(azimuth_deg)
    phi += np.radians(elevation_deg)

    # Convert back to Cartesian
    new_pos = np.array(
        [
            distance * np.cos(phi) * np.cos(theta),
            distance * np.cos(phi) * np.sin(theta),
            distance * np.sin(phi),
        ]
    )

    return target + new_pos
