#!/bin/sh
# meshtui installer: installs the binary and a reference copy of the default
# config into $PREFIX (default ~/.local). No root needed for user installs.
#
#   ./install.sh                  # install to ~/.local
#   PREFIX=/usr/local sudo -E ./install.sh
set -eu

PREFIX="${PREFIX:-$HOME/.local}"

if [ ! -f meshtui ]; then
    echo "install.sh: run me from the extracted release tarball (meshtui binary missing)" >&2
    exit 1
fi

mkdir -p "$PREFIX/bin" "$PREFIX/share/meshtui"
install -Dm755 meshtui "$PREFIX/bin/meshtui"
if [ -f default_config.toml ]; then
    install -Dm644 default_config.toml "$PREFIX/share/meshtui/default_config.toml"
fi

case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "note: add $PREFIX/bin to your PATH" ;;
esac
echo "meshtui installed to $PREFIX/bin/meshtui"
