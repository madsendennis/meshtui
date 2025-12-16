"""Mesh loading utilities using trimesh."""

from pathlib import Path

import numpy as np
import trimesh


def load_mesh(file_path: Path) -> trimesh.Trimesh:
    """Load a mesh from a file.

    Loads and processes a mesh file, centering and normalizing it for display.

    Args:
        file_path: Path to the mesh file (.ply or .stl)

    Returns:
        Loaded and processed trimesh.Trimesh object

    Raises:
        FileNotFoundError: If file does not exist
        ValueError: If file format is not supported or file cannot be loaded
    """
    # Validate file exists
    if not file_path.exists():
        raise FileNotFoundError(f"File not found: {file_path}")

    # Validate file extension
    supported_extensions = {".ply", ".stl"}
    if file_path.suffix.lower() not in supported_extensions:
        raise ValueError(
            f"Unsupported file format: {file_path.suffix}. "
            f"Supported formats: {', '.join(supported_extensions)}"
        )

    # Load mesh
    try:
        mesh = trimesh.load(str(file_path))
    except Exception as e:
        raise ValueError(f"Failed to load mesh from {file_path}: {e}") from e

    # Ensure we have a Trimesh object (not a Scene)
    if isinstance(mesh, trimesh.Scene):
        # Extract the first geometry from the scene
        if len(mesh.geometry) == 0:
            raise ValueError(f"No geometry found in {file_path}")
        mesh = list(mesh.geometry.values())[0]

    if not isinstance(mesh, trimesh.Trimesh):
        raise ValueError(f"Loaded object is not a valid mesh: {type(mesh)}")

    # Center the mesh at origin
    mesh.vertices -= mesh.centroid

    # Normalize to unit scale (fit in 1x1x1 box)
    bounds = mesh.bounds
    scale = np.max(bounds[1] - bounds[0])
    if scale > 0:
        mesh.vertices /= scale

    return mesh
