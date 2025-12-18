# MeshTUI Project Setup and Implementation Plan

## Project Overview
A terminal-based mesh viewer using the Kitty image protocol to display 3D meshes (.ply, .stl, .obj, .drc, .glb) directly in the terminal. Supports multiple view angles, zoom, smooth transitions, and user customization via config files. Future extensions include support for multiple meshes and a full-featured TUI similar to MeshLab but focused on viewing capabilities.

## Phase 1: Initial Setup and Basic Viewer ✅ (Completed)

### Goal
Create a simple CLI tool that can be invoked as `meshtui /path/to/meshfile.ply` and display a single-frame rendering of the mesh in the terminal.

### Technology Stack
- **Language:** Python 3.12+
- **Package Manager:** UV
- **Mesh Processing:** trimesh
- **Rendering:** pygfx - Modern WebGPU-based offscreen rendering
- **Display Protocol:** Kitty image protocol

---

## Detailed Implementation Steps

### Step 1: Project Initialization
- [x] Initialize UV project structure
- [x] Create `pyproject.toml` with project metadata
- [x] Set up basic directory structure:
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
- [x] Configure mypy in `pyproject.toml` (basic config: check function signatures only, allow reassignment)
- [x] Set up `.pre-commit-config.yaml` with black, ruff, and mypy hooks
- [x] Initialize pre-commit hooks with `pre-commit install`

### Step 2: Dependency Management
Define dependencies in `pyproject.toml`:
- **Core dependencies:**
  - `trimesh` - Mesh loading and manipulation
  - `numpy` - Numerical operations
  - `pillow` (PIL) - Image generation
  - `pygfx` - Modern WebGPU-based rendering with offscreen support
  - `rendercanvas` - Rendering canvas for pygfx
  - `scipy` - Scientific computing utilities

- **Development dependencies:**
  - `pytest` - Testing
  - `black` - Code formatting
  - `ruff` - Linting
  - `mypy` - Type checking (basic function signatures only)
  - `pre-commit` - Git pre-commit hooks

### Step 2.5: Configuration System
- [x] Create configuration module for user customization
  - [x] Load default configuration from `default_config.toml`
  - [x] Support user config file override (e.g., `~/.config/meshtui/config.toml`)
  - [x] Configure rendering settings (camera, lighting, materials)
  - [x] Configure display settings (terminal dimensions, image format)
  - [x] Deep merge user config with defaults

### Step 4: Terminal Detection and Kitty Protocol
- [x] Create module to detect terminal dimensions
  - [x] Use Kitty-specific window size query (escape sequence for pixel dimensions)
  - [x] Query: `\033[14t` for cell size or Kitty's graphics protocol query
  - [x] Parse response to get actual pixel dimensions and cell size
  - [x] Fallback to `os.get_terminal_size()` for basic dimensions
  - [x] Calculate available space for image

- [x] Implement Kitty image protocol handler
  - [x] Implement Kitty graphics protocol escape sequences
  - [x] Implement base64 encoding for image data (PNG format)
  - [x] Create function to output images using protocol
  - [x] Handle fallback for non-Kitty terminals (error message or basic ASCII art)

### Step 5: Mesh Loading
- [x] Create mesh loader module using trimesh
  - [x] Support .ply format
  - [x] Support .stl format
  - [x] Support .obj format
  - [x] Support .drc format
  - [x] Support .glb format
  - [x] Handle file validation
  - [x] Extract mesh properties (vertices, faces, bounds)
  - [x] Center and normalize mesh for consistent viewing

### Step 6: Rendering Pipeline
- [x] Create renderer module using pygfx
  - [x] Set up pygfx offscreen renderer with WebGPU backend
  - [x] Create scene and add mesh with material/texture support (default materials)
  - [x] Implement camera setup (perspective projection with multiple view angles)
  - [x] Add zoom functionality
  - [x] Calculate appropriate view angle to show entire mesh (use mesh bounds)
  - [x] Add lighting (directional + ambient light)
  - [x] Configure renderer with terminal dimensions
  - [x] Render scene to numpy array / PIL Image
  - [x] Support smooth transitions between views

### Step 7: CLI Interface [x]
- [x] Create main entry point
  - Parse command-line arguments (mesh file path)
  - Validate file exists and format is supported
  - Coordinate mesh loading → rendering → display pipeline
  - Add comprehensive error handling
  - Handle errors gracefully with informative messages

### Step 7.5: Smooth Transitions
- [x] Implement smooth camera transitions between view angles
  - [x] Animate camera movements for better user experience
  - [x] Support configurable transition speeds
  - [x] Ensure transitions work with zoom functionality

### Step 8: Testing and Refinement
- [x] Write minimal tests for maximum coverage
  - [x] Use mock meshes (programmatically generated with trimesh/numpy)
  - [x] Mock file I/O operations to avoid disk access
  - [x] Create simple test meshes (cubes, pyramids) in-memory
  - [x] Mock Kitty protocol responses for terminal size queries
  - [x] Test error handling with invalid inputs
- [x] Verify terminal dimension detection (with mocked responses)
- [x] Test rendering pipeline (with simple generated meshes)
- [x] Handle edge cases (very large meshes, corrupt files, unsupported formats)
- [x] Ensure tests run quickly (no disk I/O, no actual file loading)

---

## Phase 2: Multiple Mesh Support

### Goal
Extend the viewer to support loading and displaying multiple meshes simultaneously.

### Implementation Steps

#### Step 1: Support Explicit Mesh List Input
- [ ] Allow passing multiple mesh file paths as arguments
- [ ] Implement batch loading of specified meshes
- [ ] Display meshes in a carousel or grid view
- [ ] Add keyboard shortcuts for navigation between meshes

#### Step 2: Support Loading Multiple Meshes from Directory
- [ ] Modify CLI to accept directory path as input
- [ ] Automatically discover all supported mesh files in directory
- [ ] Load all meshes and display them in sequence or grid layout
- [ ] Add navigation controls to switch between meshes

---

## Phase 3: Future Enhancements (Not Implemented Initially)
- Interactive TUI with file browser
- Mesh information display (vertex count, face count, bounds)
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
**Using Pygfx:** Modern WebGPU-based rendering with offscreen support
- Pros: High-quality output, fast rendering, excellent control, cross-platform WebGPU backend
- Setup: Use pygfx with rendercanvas for offscreen rendering
- Future: Can map controls for interactive TUI phase
- Note: Uses WebGPU instead of OpenGL for better performance and compatibility

**Recommendation:** Pygfx provides modern rendering capabilities with WebGPU backend.

### UV Project Management
UV is a modern Python package manager that's fast and reliable:
- `uv init` - Initialize project
- `uv add <package>` - Add dependency
- `uv sync` - Install dependencies
- `uv run <command>` - Run in project environment

---

## Success Criteria for Phase 1
- [x] Command `meshtui mesh.ply` displays the mesh in a Kitty-compatible terminal
- [x] Mesh is properly scaled and centered in view
- [x] Terminal dimensions are detected and respected
- [x] Supports .ply, .stl, .obj, .drc, and .glb formats
- [x] Multiple view angles and zoom functionality
- [x] Material and texture support with defaults
- [x] Smooth transitions between views
- [x] User-configurable settings via config file
- [x] Clear error messages for unsupported terminals or invalid files
- [x] Code is well-structured and documented

---

## Estimated Timeline (Phase 1 Completed)
- Project setup: 30 minutes ✅
- Kitty protocol implementation: 1-2 hours ✅
- Mesh loading: 30 minutes ✅
- Rendering pipeline: 2-3 hours ✅
- CLI and integration: 1 hour ✅
- Testing and refinement: 1-2 hours ✅
- Configuration system: 1 hour ✅
- Smooth transitions: 1 hour ✅

**Total: ~8-10 hours of development time (Completed)**

---

## Next Steps
1. Review and validate this plan
2. Initialize UV project structure
3. Set up dependencies
4. Begin implementation following the steps above
