"""Main entry point for meshtui CLI."""

import sys
from pathlib import Path


def main() -> int:
    """Main entry point for meshtui CLI."""
    if len(sys.argv) < 2:
        print("Usage: meshtui <mesh_file>")
        return 1

    mesh_path = Path(sys.argv[1])
    if not mesh_path.exists():
        print(f"Error: File not found: {mesh_path}")
        return 1

    print(f"Loading mesh: {mesh_path}")
    # TODO: Implement mesh loading and rendering
    return 0


if __name__ == "__main__":
    sys.exit(main())
