#!/usr/bin/env python3
"""Generate test meshes for meshtui into testdata/meshes/.

Stdlib only. Covers the loader feature matrix:

  STL  cube_ascii.stl, cube_binary.stl
  PLY  sphere_ascii.ply, cylinder_binary_le.ply, cube_binary_be.ply,
       cube_vertex_colors_ascii.ply, torus_colors_alpha_binary.ply
  OBJ  cube_quads_multi.obj (quads, groups, negative indices),
       cube_normals.obj, cube_textured.obj (+ .mtl + checker.png;
       textures are ignored by the loader, geometry must still load)
  GLB  cube_plain.glb, cube_vertex_colors.glb, two_materials.glb

The .drc files and cube_draco.glb need a Draco encoder and are written by
`cargo run -p meshtui-core --example gen_testdata_draco` instead.
"""

import json
import math
import struct
import zlib
from pathlib import Path

OUT = Path(__file__).parent / "meshes"


# ---------------------------------------------------------------- geometry
def cube():
    """Unit cube centred at the origin: (positions, quad faces)."""
    p = [(x, y, z) for x in (-0.5, 0.5) for y in (-0.5, 0.5) for z in (-0.5, 0.5)]
    quads = [
        (0, 1, 3, 2), (4, 6, 7, 5),  # z- / z+
        (0, 4, 5, 1), (2, 3, 7, 6),  # y- / y+
        (0, 2, 6, 4), (1, 5, 7, 3),  # x- / x+
    ]
    return p, quads


def sphere(radius=0.6, seg=24, rings=12):
    p = [(0.0, radius, 0.0)]
    for r in range(1, rings):
        phi = math.pi * r / rings
        for s in range(seg):
            theta = 2 * math.pi * s / seg
            p.append((radius * math.sin(phi) * math.cos(theta),
                      radius * math.cos(phi),
                      radius * math.sin(phi) * math.sin(theta)))
    p.append((0.0, -radius, 0.0))
    south = len(p) - 1
    tris = [(0, 1 + s, 1 + (s + 1) % seg) for s in range(seg)]
    for r in range(rings - 2):
        a0, b0 = 1 + r * seg, 1 + (r + 1) * seg
        for s in range(seg):
            sn = (s + 1) % seg
            tris += [(a0 + s, b0 + s, b0 + sn), (a0 + s, b0 + sn, a0 + sn)]
    tris += [(south, 1 + (rings - 2) * seg + (s + 1) % seg,
              1 + (rings - 2) * seg + s) for s in range(seg)]
    return p, tris


def cylinder(radius=0.4, height=1.0, seg=24):
    p = [(radius * math.cos(2 * math.pi * s / seg), h,
          radius * math.sin(2 * math.pi * s / seg))
         for h in (height / 2, -height / 2) for s in range(seg)]
    tris = []
    for s in range(seg):
        sn = (s + 1) % seg
        tris += [(s, seg + s, seg + sn), (s, seg + sn, sn)]
    top, bot = len(p), len(p) + 1
    p += [(0.0, height / 2, 0.0), (0.0, -height / 2, 0.0)]
    tris += [(top, (s + 1) % seg, s) for s in range(seg)]
    tris += [(bot, seg + s, seg + (s + 1) % seg) for s in range(seg)]
    return p, tris


def torus(major=0.5, minor=0.2, seg=24, rings=12):
    p = []
    for r in range(rings):
        phi = 2 * math.pi * r / rings
        for s in range(seg):
            theta = 2 * math.pi * s / seg
            rad = major + minor * math.cos(theta)
            p.append((rad * math.cos(phi), minor * math.sin(theta),
                      rad * math.sin(phi)))
    tris = []
    for r in range(rings):
        for s in range(seg):
            a = r * seg + s
            b = r * seg + (s + 1) % seg
            c = ((r + 1) % rings) * seg + s
            d = ((r + 1) % rings) * seg + (s + 1) % seg
            tris += [(a, c, d), (a, d, b)]
    return p, tris


def face_normal(a, b, c):
    ux, uy, uz = (b[i] - a[i] for i in range(3))
    vx, vy, vz = (c[i] - a[i] for i in range(3))
    n = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx)
    length = math.sqrt(sum(x * x for x in n)) or 1.0
    return tuple(x / length for x in n)


# ---------------------------------------------------------------- STL
def write_stl_ascii(path, positions, tris, name="solid"):
    with open(path, "w") as f:
        f.write(f"solid {name}\n")
        for t in tris:
            n = face_normal(*(positions[i] for i in t))
            f.write(f"  facet normal {n[0]:e} {n[1]:e} {n[2]:e}\n")
            f.write("    outer loop\n")
            for i in t:
                f.write(f"      vertex {positions[i][0]:e} {positions[i][1]:e}"
                        f" {positions[i][2]:e}\n")
            f.write("    endloop\n  endfacet\n")
        f.write(f"endsolid {name}\n")


def write_stl_binary(path, positions, tris):
    with open(path, "wb") as f:
        f.write(b"meshtui test binary STL".ljust(80, b"\0"))
        f.write(struct.pack("<I", len(tris)))
        for t in tris:
            n = face_normal(*(positions[i] for i in t))
            f.write(struct.pack("<3f", *n))
            for i in t:
                f.write(struct.pack("<3f", *positions[i]))
            f.write(struct.pack("<H", 0))


# ---------------------------------------------------------------- PLY
def write_ply_ascii(path, positions, tris, colors=None):
    with open(path, "w") as f:
        f.write("ply\nformat ascii 1.0\ncomment meshtui test data\n")
        f.write(f"element vertex {len(positions)}\n")
        f.write("property float x\nproperty float y\nproperty float z\n")
        if colors:
            f.write("property uchar red\nproperty uchar green\n"
                    "property uchar blue\n")
        f.write(f"element face {len(tris)}\n")
        f.write("property list uchar int vertex_indices\nend_header\n")
        for i, p in enumerate(positions):
            row = f"{p[0]} {p[1]} {p[2]}"
            if colors:
                row += f" {colors[i][0]} {colors[i][1]} {colors[i][2]}"
            f.write(row + "\n")
        for t in tris:
            f.write(f"3 {t[0]} {t[1]} {t[2]}\n")


def write_ply_binary(path, positions, tris, big_endian=False, colors=None,
                     alpha=False):
    e = ">" if big_endian else "<"
    fmt = "binary_big_endian" if big_endian else "binary_little_endian"
    header = f"ply\nformat {fmt} 1.0\nelement vertex {len(positions)}\n"
    header += "property float x\nproperty float y\nproperty float z\n"
    if colors:
        header += "property uchar red\nproperty uchar green\nproperty uchar blue\n"
        if alpha:
            header += "property uchar alpha\n"
    header += f"element face {len(tris)}\n"
    header += "property list uchar int vertex_indices\nend_header\n"
    with open(path, "wb") as f:
        f.write(header.encode())
        for i, p in enumerate(positions):
            f.write(struct.pack(f"{e}3f", *p))
            if colors:
                f.write(struct.pack("4B" if alpha else "3B", *colors[i]))
        for t in tris:
            f.write(struct.pack(f"{e}B3i", 3, *t))


def rainbow_colors(positions, alpha=False):
    out = []
    ys = [p[1] for p in positions]
    lo, hi = min(ys), max(ys)
    span = (hi - lo) or 1.0
    for x, y, z in positions:
        t = (y - lo) / span
        r = int(255 * t)
        b = 255 - r
        g = int(255 * abs(x + z) / (abs(x) + abs(z) + 1e-9))
        out.append((r, g, b, 255) if alpha else (r, g, b))
    return out


# ---------------------------------------------------------------- OBJ
def write_obj_quads_multi(path):
    """Two grouped cubes with quad faces; third group uses negative indices."""
    p, quads = cube()
    p2 = [(x + 1.5, y, z) for x, y, z in p]
    n = len(p) + len(p2)
    with open(path, "w") as f:
        f.write("# meshtui test: quads + groups + negative indices\n")
        f.write("g first\n")
        for v in p + p2:
            f.write(f"v {v[0]} {v[1]} {v[2]}\n")
        for q in quads:
            f.write(f"f {q[0]+1} {q[1]+1} {q[2]+1} {q[3]+1}\n")
        f.write("g second\n")
        for q in quads:
            f.write(f"f {q[0]+9} {q[1]+9} {q[2]+9} {q[3]+9}\n")
        # reuses the second cube's vertices via negative (relative) indices
        f.write("g negative\n")
        for q in quads[:2]:
            f.write("f " + " ".join(str(v + 9 - 1 - n) for v in q) + "\n")


def write_obj_normals(path):
    p, quads = cube()
    tris = [(q[0], q[1], q[2]) for q in quads] + [(q[0], q[2], q[3]) for q in quads]
    normals = [face_normal(*(p[i] for i in t)) for t in tris]
    with open(path, "w") as f:
        f.write("# meshtui test: authored normals\n")
        for v in p:
            f.write(f"v {v[0]} {v[1]} {v[2]}\n")
        for n in normals:
            f.write(f"vn {n[0]} {n[1]} {n[2]}\n")
        for ti, t in enumerate(tris):
            f.write(f"f {t[0]+1}//{ti+1} {t[1]+1}//{ti+1} {t[2]+1}//{ti+1}\n")


def write_png_checkerboard(path, size=16, cells=4):
    """Minimal RGB PNG: black/white checkerboard."""
    cell = size // cells
    rows = b""
    for y in range(size):
        rows += b"\x00"  # filter: none
        for x in range(size):
            on = (x // cell + y // cell) % 2
            rows += bytes((255, 255, 255) if on else (20, 20, 20))

    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(rows))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)


def write_obj_textured(path):
    """Textured cube: mtllib/usemtl/vt — ignored by the loader today."""
    p, quads = cube()
    uv = [(0, 0), (1, 0), (1, 1), (0, 1)]
    write_png_checkerboard(path.with_name("checker.png"))
    path.with_name("cube_textured.mtl").write_text(
        "newmtl checker\n"
        "Kd 1.0 1.0 1.0\n"
        "map_Kd checker.png\n")
    with open(path, "w") as f:
        f.write("mtllib cube_textured.mtl\n")
        f.write("o textured_cube\n")
        for v in p:
            f.write(f"v {v[0]} {v[1]} {v[2]}\n")
        for t in uv:
            f.write(f"vt {t[0]} {t[1]}\n")
        f.write("usemtl checker\n")
        for q in quads:
            f.write("f " + " ".join(f"{vi+1}/{ti+1}"
                                    for ti, vi in enumerate(q)) + "\n")


# ---------------------------------------------------------------- GLB
def glb(json_obj, bin_blob):
    js = json.dumps(json_obj, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    bin_blob += b"\0" * (-len(bin_blob) % 4)
    total = 12 + 8 + len(js) + 8 + len(bin_blob)
    return (struct.pack("<III", 0x46546C67, 2, total)
            + struct.pack("<II", len(js), 0x4E4F534A) + js
            + struct.pack("<II", len(bin_blob), 0x004E4942) + bin_blob)


def mesh_glb(path, meshes):
    """meshes: list of dicts with keys positions, tris, optional normals,
    colors (u8 rgba per vertex), material (baseColorFactor rgba)."""
    blob = b""
    views, accessors, out_meshes, materials = [], [], [], []

    def add_view(data, target):
        nonlocal blob
        views.append({"buffer": 0, "byteOffset": len(blob),
                      "byteLength": len(data), "target": target})
        blob += data
        return len(views) - 1

    def add_accessor(view, comp, count, atype, **kw):
        accessors.append({"bufferView": view, "componentType": comp,
                          "count": count, "type": atype, **kw})
        return len(accessors) - 1

    for mi, m in enumerate(meshes):
        p, tris = m["positions"], m["tris"]
        pdata = b"".join(struct.pack("<3f", *v) for v in p)
        pos = add_accessor(add_view(pdata, 34962), 5126, len(p), "VEC3",
                           min=[min(v[i] for v in p) for i in range(3)],
                           max=[max(v[i] for v in p) for i in range(3)])
        idata = b"".join(struct.pack("<I", i) for t in tris for i in t)
        idx = add_accessor(add_view(idata, 34963), 5125, len(tris) * 3,
                           "SCALAR")
        attrs = {"POSITION": pos}
        if "normals" in m:
            ndata = b"".join(struct.pack("<3f", *n) for n in m["normals"])
            attrs["NORMAL"] = add_accessor(add_view(ndata, 34962), 5126,
                                           len(m["normals"]), "VEC3")
        if "colors" in m:
            cdata = bytes(c for rgba in m["colors"] for c in rgba)
            attrs["COLOR_0"] = add_accessor(add_view(cdata, 34962), 5121,
                                            len(m["colors"]), "VEC4",
                                            normalized=True)
        prim = {"attributes": attrs, "indices": idx}
        if "material" in m:
            materials.append({"pbrMetallicRoughness":
                              {"baseColorFactor": m["material"]}})
            prim["material"] = len(materials) - 1
        out_meshes.append({"name": m.get("name", f"mesh{mi}"),
                           "primitives": [prim]})

    doc = {"asset": {"version": "2.0", "generator": "meshtui testdata"},
           "scene": 0,
           "scenes": [{"nodes": list(range(len(out_meshes)))}],
           "nodes": [{"mesh": i} for i in range(len(out_meshes))],
           "meshes": out_meshes,
           "buffers": [{"byteLength": len(blob)}],
           "bufferViews": views,
           "accessors": accessors}
    if materials:
        doc["materials"] = materials
    path.write_bytes(glb(doc, blob))


def vertex_normals(positions, tris):
    acc = [[0.0, 0.0, 0.0] for _ in positions]
    for t in tris:
        n = face_normal(*(positions[i] for i in t))
        for i in t:
            for k in range(3):
                acc[i][k] += n[k]
    out = []
    for a in acc:
        length = math.sqrt(sum(x * x for x in a)) or 1.0
        out.append(tuple(x / length for x in a))
    return out


# ---------------------------------------------------------------- main
def main():
    OUT.mkdir(parents=True, exist_ok=True)
    p, quads = cube()
    tris = [(q[0], q[1], q[2]) for q in quads] + [(q[0], q[2], q[3]) for q in quads]

    # STL
    write_stl_ascii(OUT / "cube_ascii.stl", p, tris)
    write_stl_binary(OUT / "cube_binary.stl", p, tris)

    # PLY
    sp, st = sphere()
    write_ply_ascii(OUT / "sphere_ascii.ply", sp, st)
    cp, ct = cylinder()
    write_ply_binary(OUT / "cylinder_binary_le.ply", cp, ct)
    write_ply_binary(OUT / "cube_binary_be.ply", p, tris, big_endian=True)
    write_ply_ascii(OUT / "cube_vertex_colors_ascii.ply", p, tris,
                    colors=rainbow_colors(p))
    tp, tt = torus()
    write_ply_binary(OUT / "torus_colors_alpha_binary.ply", tp, tt,
                     colors=rainbow_colors(tp, alpha=True), alpha=True)

    # OBJ
    write_obj_quads_multi(OUT / "cube_quads_multi.obj")
    write_obj_normals(OUT / "cube_normals.obj")
    write_obj_textured(OUT / "cube_textured.obj")

    # GLB
    normals = vertex_normals(p, tris)
    mesh_glb(OUT / "cube_plain.glb",
             [{"positions": p, "tris": tris, "normals": normals,
               "name": "cube"}])
    rgba = [c + (255,) if len(c) == 3 else c for c in rainbow_colors(p)]
    mesh_glb(OUT / "cube_vertex_colors.glb",
             [{"positions": p, "tris": tris, "normals": normals,
               "colors": rgba, "name": "colored_cube"}])
    p2 = [(x + 1.5, y, z) for x, y, z in p]
    mesh_glb(OUT / "two_materials.glb", [
        {"positions": p, "tris": tris, "normals": normals, "name": "red",
         "material": [0.9, 0.1, 0.1, 1.0]},
        {"positions": p2, "tris": tris, "normals": normals, "name": "blue",
         "material": [0.1, 0.2, 0.9, 1.0]},
    ])

    print(f"wrote {len(list(OUT.iterdir()))} files to {OUT}")


if __name__ == "__main__":
    main()
