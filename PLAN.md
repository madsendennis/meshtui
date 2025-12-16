# MeshTUI Project Setup and Implementation Plan

## Project Overview
A terminal-based mesh viewer using the Kitty image protocol to display 3D meshes (.ply and .stl) directly in the terminal. Future extensions will include a full-featured TUI similar to MeshLab but focused on viewing capabilities.

## Phase 1: Initial Setup and Basic Viewer (Current Focus)

### Goal
Create a simple CLI tool that can be invoked as `meshtui /path/to/meshfile.ply` and display a single-frame rendering of the mesh in the terminal.

### Technology Stack
- **Language:** Python 3.12+
- **Package Manager:** UV
- **Mesh Processing:** trimesh
- **Rendering:** pyrender - Modern OpenGL-based offscreen rendering
- **Display Protocol:** Kitty image protocol

---

## Detailed Implementation Steps

### Step 1: Project Initialization
- [ ] Initialize UV project structure
- [ ] Create `pyproject.toml` with project metadata
- [ ] Set up basic directory structure:
  ```
  meshtui/
  ├── src/
  │   └── meshtui/
  │       ├── __init__.py
  │       ├── main.py
  │       ├── mesh_loader.py
  │       ├── renderer.py
  │       └── kitty_protocol.py
  ├── tests/
  │   └── __init__.py
  ├── .pre-commit-config.yaml
  ├── .gitignore
  ├── pyproject.toml
  ├── README.md
  └── PLAN.md
  ```
- [ ] Configure mypy in `pyproject.toml` (basic config: check function signatures only, allow reassignment)
- [ ] Set up `.pre-commit-config.yaml` with black, ruff, and mypy hooks
- [ ] Initialize pre-commit hooks with `pre-commit install`

### Step 2: Dependency Management
Define dependencies in `pyproject.toml`:
- **Core dependencies:**
  - `trimesh` - Mesh loading and manipulation
  - `numpy` - Numerical operations
  - `pillow` (PIL) - Image generation
  - `pyrender` - Modern OpenGL-based rendering with offscreen support
  - `pyglet` - For offscreen rendering context (required by pyrender)

- **Development dependencies:**
  - `pytest` - Testing
  - `black` - Code formatting
  - `ruff` - Linting
  - `mypy` - Type checking (basic function signatures only)
  - `pre-commit` - Git pre-commit hooks

### Step 3: Terminal Detection and Kitty Protocol
- [ ] Create module to detect terminal dimensions
  - Use Kitty-specific window size query (escape sequence for pixel dimensions)
  - Query: `\033[14t` for cell size or Kitty's graphics protocol query
  - Parse response to get actual pixel dimensions and cell size
  - Fallback to `os.get_terminal_size()` for basic dimensions
  - Calculate available space for image

- [ ] Implement Kitty image protocol handler
  - Implement Kitty graphics protocol escape sequences
  - Implement base64 encoding for image data (PNG format)
  - Create function to output images using protocol
  - Handle fallback for non-Kitty terminals (error message or basic ASCII art)

### Step 4: Mesh Loading
- [ ] Create mesh loader module using trimesh
  - Support .ply format
  - Support .stl format
  - Handle file validation
  - Extract mesh properties (vertices, faces, bounds)
  - Center and normalize mesh for consistent viewing

### Step 5: Rendering Pipeline
- [ ] Create renderer module using pyrender
  - Set up pyrender offscreen renderer (OSMesa or EGL backend)
  - Create scene and add mesh
  - Implement camera setup (perspective projection)
  - Calculate appropriate view angle to show entire mesh (use mesh bounds)
  - Add lighting (directional + ambient light)
  - Configure renderer with terminal dimensions
  - Render scene to numpy array / PIL Image
  - Reference pyrender.Viewer for future interactive controls mapping

### Step 6: CLI Interface
- [ ] Create main entry point
  - Parse command-line arguments (mesh file path)
  - Validate file exists and format is supported
  - Coordinate mesh loading → rendering → display pipeline
  - Handle errors gracefully with informative messages

### Step 7: Testing and Refinement
- [ ] Write minimal tests for maximum coverage
  - Use mock meshes (programmatically generated with trimesh/numpy)
  - Mock file I/O operations to avoid disk access
  - Create simple test meshes (cubes, pyramids) in-memory
  - Mock Kitty protocol responses for terminal size queries
  - Test error handling with invalid inputs
- [ ] Verify terminal dimension detection (with mocked responses)
- [ ] Test rendering pipeline (with simple generated meshes)
- [ ] Handle edge cases (very large meshes, corrupt files, unsupported formats)
- [ ] Ensure tests run quickly (no disk I/O, no actual file loading)

---

## Phase 2: Future Enhancements (Not Implemented Initially)
- Interactive TUI with file browser
- Multiple view angles and zoom
- Mesh information display (vertex count, face count, bounds)
- Support for additional mesh formats
- Material and texture support
- Multiple mesh viewing
- Side-by-side mesh comparison with synchronized camera views
  - Display two meshes simultaneously
  - Synchronized camera movements (rotation, zoom, pan)
  - Enable easy visual comparison of mesh differences/similarities
- Yazi file explorer integration for mesh preview on hover

---

## Technical Considerations

### Kitty Image Protocol
The Kitty terminal graphics protocol allows displaying images directly in the terminal using escape sequences. Key points:
- Images are transmitted as base64-encoded PNG/RGB data
- Protocol supports chunked transmission for large images
- Format: `\033_G<control_data>;<base64_data>\033\\`
- Window size query: Use Kitty's graphics protocol to get exact pixel dimensions
  - Reference: https://sw.kovidgoyal.net/kitty/graphics-protocol/#getting-the-window-size
  - Provides accurate pixel dimensions and cell size for optimal image rendering
- Need to calculate proper size based on terminal cell dimensions
**Using Pyrender:** Modern OpenGL-based rendering with offscreen support
- Pros: High-quality output, fast rendering, excellent control, built-in viewer with hotkeys to reference
- Setup: Use pyrender.OffscreenRenderer with OSMesa or EGL backend
- Future: Can map pyrender.Viewer hotkeys for interactive TUI phase
- Note: Requires OpenGL context (pyglet provides this for offscreen rendering)

**Recommendation:** Start with matplotlib for simplicity, migrate to pyrender if needed.

### UV Project Management
UV is a modern Python package manager that's fast and reliable:
- `uv init` - Initialize project
- `uv add <package>` - Add dependency
- `uv sync` - Install dependencies
- `uv run <command>` - Run in project environment

---

## Success Criteria for Phase 1
- [ ] Command `meshtui mesh.ply` displays the mesh in a Kitty-compatible terminal
- [ ] Mesh is properly scaled and centered in view
- [ ] Terminal dimensions are detected and respected
- [ ] Supports both .ply and .stl formats
- [ ] Clear error messages for unsupported terminals or invalid files
- [ ] Code is well-structured and documented

---

## Estimated Timeline
- Project setup: 30 minutes
- Kitty protocol implementation: 1-2 hours
- Mesh loading: 30 minutes
- Rendering pipeline: 2-3 hours
- CLI and integration: 1 hour
- Testing and refinement: 1-2 hours

**Total: 6-9 hours of development time**

---

## Next Steps
1. Review and validate this plan
2. Initialize UV project structure
3. Set up dependencies
4. Begin implementation following the steps above
