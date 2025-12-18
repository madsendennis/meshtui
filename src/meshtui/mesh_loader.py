"""Mesh loading utilities using trimesh."""

from pathlib import Path

import trimesh


def load_mesh(file_path: Path) -> trimesh.Trimesh:
    """Load a mesh from a file.

    Loads a mesh file without altering its world-space coordinates.

    Args:
        file_path: Path to the mesh file (.ply, .stl, .obj, .drc, .glb)

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
    supported_extensions = {".ply", ".stl", ".obj", ".drc", ".glb"}
    if file_path.suffix.lower() not in supported_extensions:
        raise ValueError(
            f"Unsupported file format: {file_path.suffix}. "
            f"Supported formats: {', '.join(sorted(supported_extensions))}"
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

    return mesh
