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

# Offer to install the agent skill (SKILL.md) so coding agents discover the
# tool. The skill just points the agent at `meshtui --capabilities`, so it
# never goes stale. Skip in non-interactive (piped/scripted) installs.
if [ -t 0 ]; then
    SKILLS_DIR=""
    for d in "$HOME/.agents/skills" "$HOME/.config/agents/skills"; do
        if [ -d "$d" ]; then SKILLS_DIR="$d"; break; fi
    done
    : "${SKILLS_DIR:=$HOME/.agents/skills}"
    printf 'Install the agent skill to %s/meshtui/SKILL.md? [y/N] ' "$SKILLS_DIR"
    read -r ans || ans=""
    case "$ans" in
        y | Y | yes)
            mkdir -p "$SKILLS_DIR/meshtui"
            "$PREFIX/bin/meshtui" skill >"$SKILLS_DIR/meshtui/SKILL.md"
            echo "skill installed to $SKILLS_DIR/meshtui/SKILL.md"
            ;;
        *) echo "skipped (run 'meshtui skill > <skills-dir>/meshtui/SKILL.md' anytime)" ;;
    esac
fi
