#!/bin/sh
# Compile the same chart through LuaLaTeX's in-process route and pdfLaTeX's shell route.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
export TEXINPUTS="$ROOT/packages/latex//:"
export LUAINPUTS="$ROOT/packages/latex//:"
export PATH="$ROOT/bin:$PATH"
cd "$ROOT/examples/latex"
for engine in lualatex pdflatex; do
  "$engine" --shell-escape -interaction=nonstopmode -halt-on-error \
    -jobname="chartgen-inline-$engine" chartgen-inline.tex >/dev/null
  echo "chartgen-inline-$engine.pdf"
done
