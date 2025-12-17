# MeshTUI

A terminal-based 3D mesh viewer using the Kitty image protocol.

## Overview

MeshTUI is a Python application that allows you to view 3D meshes directly in your terminal using the Kitty image protocol. The initial version provides a simple command-line interface for quick mesh visualization.

### Usage

**Important**: This application requires the Kitty terminal or a compatible terminal that supports the Kitty graphics protocol.

```bash
# View a mesh file
meshtui /path/to/meshfile.ply

# Works with .stl files too
meshtui model.stl
```

The viewer will:
1. Detect if running in Kitty terminal (exits with error if not)
2. Query terminal dimensions for optimal display
3. Load and process the mesh file (normalizes and centers the mesh)
4. Render a 3D view using pyrender with proper lighting
5. Display the image directly in the terminal

Example output:
```
Loading mesh: model.ply
Rendering 1523 vertices, 3042 faces...
[Image displays in terminal]
```

## Features (Phase 1)

- 🖼️ Display 3D meshes in the terminal using Kitty graphics protocol
- 📦 Support for .ply and .stl mesh formats
- 🎨 Automatic mesh centering and scaling
- 📐 Terminal dimension detection for optimal rendering
- ⚡ Built with UV for fast dependency management

## Supported Formats

- `.ply` - Polygon File Format (Stanford Triangle Format)
- `.stl` - Stereolithography Format

## Requirements

- Python 3.12 or higher
- Kitty terminal (or compatible terminal supporting Kitty graphics protocol)
- UV package manager

## Technology Stack

- **Mesh Processing:** trimesh - For loading and manipulating 3D meshes
- **Rendering:** pyrender - Modern OpenGL-based offscreen rendering
- **Image Protocol:** Kitty graphics protocol - For terminal display

## Future Plans

The project aims to evolve into a full-featured TUI (Text User Interface) with capabilities similar to MeshLab:

- 🗂️ File tree browser for managing multiple meshes
- 🔄 Interactive viewing (rotation, zoom, pan)
- 📊 Mesh information display (vertices, faces, bounds)
- 🎭 Multiple view angles and projections
- 🎨 Material and texture support
- 📏 Side-by-side mesh comparison with synchronized camera views
- 🔍 Yazi file explorer integration for mesh preview on hover

## Development

See [PLAN.md](PLAN.md) for detailed implementation plan and development roadmap.

## Installation

```bash
# Clone the repository
git clone https://github.com/madsendennis/meshtui.git
cd meshtui

# Install dependencies using UV
uv sync

# Run the viewer
uv run meshtui /path/to/mesh.ply
```

## Development Setup

To set up the project for development:

```bash
# Clone the repository
git clone https://github.com/madsendennis/meshtui.git
cd meshtui

# Install all dependencies including dev dependencies
uv sync --all-extras

# Install pre-commit hooks
uv run pre-commit install

# Run tests
uv run pytest

# Run linting and formatting
uv run ruff check .
uv run black .

# Run type checking
uv run mypy src/

# Run the CLI in development mode
uv run meshtui /path/to/mesh.ply
```

### Project Structure

```
meshtui/
├── src/
│   └── meshtui/
│       ├── __init__.py          # Package initialization
│       ├── main.py              # CLI entry point
│       ├── mesh_loader.py       # Mesh loading with trimesh
│       ├── renderer.py          # Rendering with pyrender
│       └── kitty_protocol.py    # Kitty graphics protocol
├── tests/                       # Test suite
├── .pre-commit-config.yaml      # Pre-commit hooks config
├── pyproject.toml               # Project configuration
└── README.md
```

## License

[To be determined]
