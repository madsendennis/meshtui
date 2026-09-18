---
name: meshtui
description: >-
  Inspect, render, and animate 3D meshes (PLY/STL/OBJ/DRC/GLB) from the
  terminal — no GUI. Use when a task needs to see a mesh, check its geometry,
  or produce a PNG/GIF illustration.
---

# meshtui — terminal mesh viewer & renderer

`meshtui` runs fully headless (no display needed). Every interactive feature
is reachable from the terminal, so you can look at a mesh, check its geometry,
and produce images or animations without the TUI.

**Exit codes: 0 = success, 1 = error. `--json` errors are JSON on stderr.**

## Discover the interface — don't memorize it

The tool is the source of truth and is always current. Explore it, don't
guess flags:

```bash
meshtui --capabilities          # machine-readable spec: formats, subcommands,
                                # scene-file & animation keys (start here)
meshtui <subcommand> --help     # authoritative flags for one subcommand
```

Subcommands: `info` (stats), `screenshot` (multi-view PNGs), `render` (scene
file → one PNG), `animate` (scene cuts → GIF). Run `--capabilities` to see
exactly what each takes.

## Typical flow

```bash
# 1. Understand the mesh (bounds, counts, area, volume → pick a camera)
meshtui info model.ply --json

# 2. One-image "did it render right" check (all 6 axis views in one PNG)
meshtui screenshot model.ply --views grid --out-dir shots

# 3. Set up a scene declaratively and render it (same file opens in the TUI)
meshtui render scene.yaml -o out.png

# 4. Animate: base scene + cuts (each cut holds N frames, changes only what
#    it names) into a looping GIF
meshtui animate cuts.yaml -o out.gif
```

Colors are palette names or `#RRGGBB[AA]`; per-mesh overrides use
`[name=]path[:color][:alpha][:visible]`; a directory of meshes loads as one
scene. Details: `meshtui screenshot --help` and `meshtui --capabilities`.
