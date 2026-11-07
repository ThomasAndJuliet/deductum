#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
flatpak run --no-documents-portal --filesystem=home \
    --env=FLATPAK_USER_DIR="$HOME/.local/share/flatpak" \
    --command=flatpak-builder org.flatpak.Builder "$@" --force-clean \
    build-flatpak packaging/flatpak/dev.twright.Deductum.yml
