"""Mesh loading utilities using trimesh."""

from pathlib import Path

import trimesh

SUPPORTED_EXTENSIONS = {".ply", ".stl", ".obj", ".drc", ".glb"}


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
    if file_path.suffix.lower() not in SUPPORTED_EXTENSIONS:
        raise ValueError(
            f"Unsupported file format: {file_path.suffix}. "
            f"Supported formats: {', '.join(sorted(SUPPORTED_EXTENSIONS))}"
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


def load_meshes(paths: list[Path] | Path) -> list[trimesh.Trimesh]:
    """Load one or more meshes from a list of paths or a directory.

    Args:
        paths: A single Path (file or directory) or a list of Paths.

    Returns:
        List of loaded trimesh.Trimesh objects.
    """
    mesh_list: list[trimesh.Trimesh] = []
    all_files: list[Path] = []

    # Normalize input to list
    input_paths = [paths] if isinstance(paths, Path) else paths

    for p in input_paths:
        if p.is_dir():
            # Find all supported files in directory
            files = [
                f for f in p.iterdir() if f.is_file() and f.suffix.lower() in SUPPORTED_EXTENSIONS
            ]
            # Sort for consistent order
            files.sort()
            all_files.extend(files)
        else:
            all_files.append(p)

    for p in all_files:
        try:
            mesh = load_mesh(p)
            mesh_list.append(mesh)
        except (FileNotFoundError, ValueError) as e:
            # Log warning but continue loading other files
            # For now just print to stderr, maybe use logging later
            import sys

            print(f"Warning: Skipping {p}: {e}", file=sys.stderr)

    if not mesh_list:
        raise ValueError("No valid meshes found")

    return mesh_list
