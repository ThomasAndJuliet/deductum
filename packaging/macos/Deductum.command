#!/bin/sh
# Starts Deductum with the Python bundled beside it and the user's Lean
# toolchain from elan. An app opened from Finder gets no shell PATH, so both
# are put on PATH here. Any arguments (a project folder) go to the app.
here="$(cd "$(dirname "$0")" && pwd)"
case "$(uname -m)" in
  arm64) py="$here/python/arm64" ;;
  *) py="$here/python/x86_64" ;;
esac
export PATH="$py/bin:$HOME/.elan/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
exec "$here/deductum" "$@"
