"""Camera setup utilities for mesh viewing."""

import numpy as np
import trimesh


def calculate_camera_parameters(
    meshes: list[trimesh.Trimesh],
    distance_padding: float,
) -> tuple[np.ndarray, float]:
    """Calculate scene bounds and extent for camera positioning.

    Args:
        meshes: List of meshes to view
        distance_padding: Multiplier for camera distance from scene

    Returns:
        Tuple of (center, max_extent)
    """
    # Calculate combined bounds
    combined_min = np.array([np.inf, np.inf, np.inf])
    combined_max = np.array([-np.inf, -np.inf, -np.inf])
    for mesh in meshes:
        combined_min = np.minimum(combined_min, mesh.bounds[0])
        combined_max = np.maximum(combined_max, mesh.bounds[1])

    center = (combined_min + combined_max) / 2.0
    extents = combined_max - combined_min
    max_extent = float(np.max(extents))

    return center, max_extent


def calculate_camera_distance(
    max_extent: float,
    distance_padding: float,
    fov_degrees: float,
    camera_type: str = "orthographic",
) -> float:
    """Calculate appropriate camera distance based on scene extent and camera type.

    Args:
        max_extent: Maximum extent of the scene
        distance_padding: Multiplier for camera distance
        fov_degrees: Field of view in degrees (for perspective camera)
        camera_type: Type of camera ('perspective' or 'orthographic')

    Returns:
        Camera distance from scene center
    """
    if camera_type == "perspective":
        # For perspective, use FOV to calculate distance
        fov_radians = np.radians(fov_degrees)
        tan_half_fov = float(np.tan(fov_radians / 2.0))
        if not np.isfinite(tan_half_fov) or tan_half_fov <= 0.0:
            tan_half_fov = 1e-6
        distance = (max_extent / tan_half_fov) * distance_padding
        if not np.isfinite(distance) or distance <= 0.0:
            distance = max_extent * distance_padding * 2.0
    else:
        # For orthographic, use simpler distance calculation
        distance = max_extent * distance_padding * 2.0

    return distance


def get_camera_position_and_up(
    center: np.ndarray | tuple[float, float, float],
    distance: float,
    view_axis: str,
    default_up_vector: tuple[float, float, float],
) -> tuple[tuple[float, float, float], tuple[float, float, float]]:
    """Get camera position and up vector for a given view axis.

    Args:
        center: Target center point
        distance: Distance from center
        view_axis: Camera view axis ('+x', '-x', '+y', '-y', '+z', '-z')
        default_up_vector: Default up vector to use (typically from config)

    Returns:
        Tuple of (camera_position, up_vector)
    """
    # Start with default up vector
    up_vector = default_up_vector

    # Calculate eye position based on view axis
    axis = view_axis.lower()
    if axis == "+z":
        camera_pos = (center[0], center[1], center[2] + distance)
    elif axis == "-z":
        camera_pos = (center[0], center[1], center[2] - distance)
    elif axis == "+x":
        camera_pos = (center[0] + distance, center[1], center[2])
        up_vector = (0.0, 0.0, 1.0)  # Use Z-up for X views
    elif axis == "-x":
        camera_pos = (center[0] - distance, center[1], center[2])
        up_vector = (0.0, 0.0, 1.0)  # Use Z-up for X views
    elif axis == "+y":
        camera_pos = (center[0], center[1] + distance, center[2])
    elif axis == "-y":
        camera_pos = (center[0], center[1] - distance, center[2])
    else:
        camera_pos = (center[0], center[1], center[2] + distance)

    return camera_pos, up_vector
