"""Mesh loading utilities using trimesh."""

from pathlib import Path
from typing import Any


def load_mesh(file_path: Path) -> Any:
    """Load a mesh from a file.

    Args:
        file_path: Path to the mesh file (.ply or .stl)

    Returns:
        Loaded mesh object

    Raises:
        ValueError: If file format is not supported
        FileNotFoundError: If file does not exist
    """
    # TODO: Implement with trimesh
    raise NotImplementedError("Mesh loading not yet implemented")
