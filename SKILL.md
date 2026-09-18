---
name: meshtui
description: >-
  Drive meshtui from the terminal to inspect, render, and animate 3D meshes
  (PLY/STL/OBJ/DRC/GLB). Use when a task needs to see a mesh, check geometry,
  or produce a PNG/GIF illustration without a GUI.
---

# meshtui — terminal mesh viewer & renderer

All subcommands run headless (no TTY needed) and exit 0 on success, 1 on
error. Formats: `.ply .stl .obj .drc .glb`. A directory loads every supported
mesh inside (sorted, non-recursive).

## Quick reference

| Task | Command |
|------|---------|
| Mesh stats (verts/faces/edges/bbox/area/volume) | `meshtui info <mesh...>` |
| Machine-readable stats | `meshtui info <mesh...> --json` |
| Single render | `meshtui render <scene.yaml> -o out.png` |
| Multi-view renders | `meshtui screenshot <mesh> --views all\|iso\|grid\|ring:N --out-dir DIR` |
| One-PNG 6-view contact sheet | `meshtui screenshot <mesh> --views grid -o DIR` |
| Animated GIF | `meshtui animate <cuts.yaml> -o out.gif` |

## Full scene control (screenshot flags)

Everything the TUI can do is a flag, so an agent sets up the whole scene:

```bash
meshtui screenshot \
  "gear=parts/gear.ply:color=#ff8000:alpha=0.9" \
  "base.ply:color=gray:visible=true" \
  --camera persp --azimuth 30 --elevation 20 --up 0,0,1 \
  --fov 50 --zoom 0.9 --light 1.5 --wireframe 0 \
  --background '#1a1b26' --views grid --size 1600x1000 --out-dir shots
```

- **Mesh spec** `[name=]path[:color=C][:alpha=A][:visible=B]` — per-mesh
  color (palette name or `#RRGGBB[AA]`), opacity, visibility, and an optional
  display name (used by animation cuts).
- **Camera**: `--camera ortho|persp`, `--view +x|-x|+y|-y|+z|-z`, or
  `--azimuth DEG --elevation DEG` around `--up X,Y,Z`; `--fov DEG`
  (perspective), `--zoom FACTOR` (<1 in, >1 out), `--distance D`.
- **Lighting/edges**: `--light 0..4`, `--wireframe PX` (0 = off).
- **Output**: `--background #RRGGBB[AA]` (default transparent), `--size WxH`,
  `--views all|iso|grid|ring:N`, `--out-dir DIR`, `--prefix NAME`.

## Scene file (`meshtui render scene.yaml`, or open in the TUI: `meshtui scene.yaml`)

```yaml
size: [1600, 1200]
background: "#1a1b26"        # omit or `transparent: true` for alpha
camera:
  kind: orthographic          # orthographic | perspective
  view: "+z"                  # or azimuth/elevation
  up: [0, 0, 1]               # Z-up parts
  zoom: 0.9
light: 1.0
wireframe: 0.0
meshes:
  - path: gear.ply            # `source:` also works
    name: gear
    color: "#ff8000"          # name | #RRGGBB | #RRGGBBAA
    alpha: 1.0
    visible: true
    scale: 1.0
    translate: [0, 0, 0]
```

Passing the `.yaml` as the mesh argument opens the same scene in the
interactive TUI, so an agent-prepared scene looks identical on screen.

## Animation / scene cuts (`meshtui animate cuts.yaml -o out.gif`)

Top level is the scene-file format (base scene) plus `fps`, default
`frames`, and `cuts:`. Each cut **holds N frames** and changes **only what
it names**; everything else carries over, so a camera-only move is one line.

```yaml
size: [800, 600]
background: "#1a1b26"
fps: 12
camera: { kind: orthographic, view: "+z" }
meshes:
  - { path: gear.ply, name: gear, color: "#ff8000" }
  - { path: base.ply, name: base, color: gray }
cuts:
  - {}                                   # hold the base scene 1 frame
  - frames: 8
    camera: { azimuth: 90 }              # rotate only
  - frames: 8
    camera: { azimuth: 180 }
    meshes: [{ name: gear, color: red }] # and recolor one mesh
```

- Cut fields: `frames` (hold count), `camera` (any of view/azimuth/
  elevation/up/zoom/distance/kind/fov), `meshes` (list; matched by `name`,
  with color/alpha/visible/scale/translate), `light`, `wireframe`.
- `--frames-dir DIR` writes numbered PNGs instead of a GIF.
- GIF loops; `background` or `transparent: true` is honored per frame.

## Agent tips

- Discover stats first with `meshtui info <mesh> --json` (has per-mesh
  `bounds`, `surface_area`, `signed_volume`) to choose a sensible camera.
- Verify a render's framing by piping the PNG out and reading it back.
- Prefer `--views grid` for a one-image "did this render right" check.
- `--json` errors go to stderr as JSON so scripts stay parseable.
