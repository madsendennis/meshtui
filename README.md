# MeshTUI

A terminal-based 3D mesh viewer using the Kitty image protocol.

## Overview

MeshTUI is a Python application that allows you to view 3D meshes directly in your terminal using the Kitty image protocol. The initial version provides a simple command-line interface for quick mesh visualization.

### Usage

**Important**: This application requires a terminal that supports the Kitty graphics protocol (such as Kitty, Ghostty, WezTerm, or other compatible terminals).

```bash
# View a mesh file
meshtui /path/to/meshfile.ply

# Works with .stl, .obj, .drc, and .glb files too
meshtui model.stl
meshtui model.obj
meshtui model.drc
meshtui model.glb
```

The viewer will:
1. Detect if terminal supports Kitty graphics protocol (exits with error if not)
2. Detect terminal background color (light/dark) for optimal mesh coloring
3. Query terminal dimensions for optimal display
4. Load and process the mesh file (normalizes and centers the mesh)
5. Render a 3D view using pyrender with transparent background and proper lighting
6. Display the image centered in the terminal using full terminal size
7. Automatically rerender when terminal is resized
8. Wait for user to exit (press 'q', Esc, or Ctrl+C)

Example output:
```
Loading mesh: model.ply
Rendering 1523 vertices, 3042 faces...
Terminal size: 1920x1080 pixels
[Image displays full-screen in terminal with transparent background]
Press 'q', Esc, or Ctrl+C to exit. Terminal will auto-resize.
```

## Features (Phase 1)

- 🖼️ Display 3D meshes in the terminal using Kitty graphics protocol
- 📦 Support for .ply, .stl, .obj, .drc, and .glb mesh formats
- 🎨 Automatic mesh centering and scaling
- 🌓 Smart background detection (light/dark) with adaptive mesh coloring
- 🔍 Transparent background that matches terminal theme
- 📐 Full terminal size rendering for maximum viewing area
- 🔄 Automatic rerendering on terminal resize (SIGWINCH)
- ⚡ Built with UV for fast dependency management
- 🎯 Easy exit with 'q', Esc, or Ctrl+C

## Supported Formats

- `.ply` - Polygon File Format (Stanford Triangle Format)
- `.stl` - Stereolithography Format
- `.obj` - Wavefront OBJ Format
- `.drc` - Google Draco Compressed Format
- `.glb` - glTF Binary Format

## Requirements

- Python 3.12 or higher
- A terminal that supports the Kitty graphics protocol:
  - Kitty
  - Ghostty
  - WezTerm
  - Or any other terminal with Kitty graphics protocol support
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

## License

[To be determined]
