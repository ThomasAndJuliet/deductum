#!/bin/sh
# Build the terminal as a Flatpak into build-flatpak/, a finished build
# directory ready for publication; with --install, install it as well.
#
# Regenerate packaging/flatpak/cargo-sources.json after changing dependencies:
#   uv run flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
#
# The Builder app is sandboxed: it is shown the user installation, which
# holds the Rust extension, and ~/.ssh, for chartgen's private repository.
set -eu

cd "$(dirname "$0")/.."
flatpak run --no-documents-portal --filesystem=home --filesystem=~/.ssh:ro \
    --env=FLATPAK_USER_DIR="$HOME/.local/share/flatpak" \
    --command=flatpak-builder org.flatpak.Builder "$@" --force-clean \
    build-flatpak packaging/flatpak/dev.twright.CosmicTermAgents.yml
