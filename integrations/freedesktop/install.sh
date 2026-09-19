#!/bin/sh
# meshtui freedesktop integration: makes MeshTUI the default handler for mesh
# files (STL/PLY/OBJ/DRC/GLB) so double-clicking them in a graphical file
# manager opens them in MeshTUI inside a Kitty-graphics-capable terminal.
#
#   ./install.sh                          # user install (recommended)
#   XDG_DATA_HOME=/usr/local/share sudo -E ./install.sh   # system-wide
#
# Set TERMINAL_CMD to change the launcher (default: xdg-terminal-exec, falling
# back to kitty). Examples: TERMINAL_CMD=ghostty, TERMINAL_CMD=wezterm
set -eu

DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"

if ! command -v meshtui >/dev/null 2>&1; then
    echo "install.sh: meshtui not found on PATH (install it first)" >&2
    exit 1
fi

if [ -z "${TERMINAL_CMD:-}" ]; then
    if command -v xdg-terminal-exec >/dev/null 2>&1; then
        TERMINAL_CMD=xdg-terminal-exec
    else
        TERMINAL_CMD=kitty
    fi
fi

MIME_DIR="$DATA_HOME/mime"
APP_DIR="$DATA_HOME/applications"
mkdir -p "$MIME_DIR/packages" "$APP_DIR"

cat > "$MIME_DIR/packages/meshtui-mesh.xml" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="model/stl">
    <comment>STL 3D mesh</comment>
    <glob weight="100" pattern="*.stl"/>
  </mime-type>
  <mime-type type="model/ply">
    <comment>PLY 3D mesh</comment>
    <glob weight="100" pattern="*.ply"/>
  </mime-type>
  <mime-type type="model/obj">
    <comment>Wavefront OBJ 3D mesh</comment>
    <glob weight="100" pattern="*.obj"/>
  </mime-type>
  <mime-type type="model/x-draco">
    <comment>Draco compressed 3D mesh</comment>
    <glob weight="100" pattern="*.drc"/>
  </mime-type>
</mime-info>
EOF

cat > "$APP_DIR/meshtui.desktop" <<EOF
[Desktop Entry]
Name=MeshTUI
Comment=Terminal 3D mesh viewer (Kitty graphics protocol)
Exec=$TERMINAL_CMD meshtui %f
Type=Application
Terminal=false
Categories=Graphics;3DGraphics;Viewer;
MimeType=model/stl;model/ply;model/obj;model/x-draco;model/gltf-binary;model/gltf+json;
Keywords=mesh;3d;stl;ply;obj;glb;draco;
EOF

# Glob weight 100 makes the filename rule beat content sniffing, so ASCII
# meshes (which sniff as text/plain) still get the mesh type.
update-mime-database "$MIME_DIR"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APP_DIR"
fi

for t in model/stl model/ply model/obj model/x-draco model/gltf-binary; do
    xdg-mime default meshtui.desktop "$t"
done

echo "meshtui is now the default handler for STL/PLY/OBJ/DRC/GLB files"
echo "launcher: $TERMINAL_CMD meshtui %f"
echo "restart your file manager to pick up the change"
