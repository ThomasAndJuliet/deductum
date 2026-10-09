#!/bin/sh
# Build and install the terminal as a Flatpak.
#
# Regenerate packaging/flatpak/cargo-sources.json after changing dependencies:
#   uv run flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
set -eu

cd "$(dirname "$0")/.."
flatpak-builder --user --install --force-clean \
    build-flatpak packaging/flatpak/dev.twright.CosmicTermAgents.yml
