# Experimental

Crates that have graduated from a sandbox but whose overall shape is still
unsettled. They are fit to use, and may change or break. A crate moves to
`crates/` once its shape is settled (no approval needed). See `AGENTS.md`.

- `figkit-raster` — PNG output for figkit scenes: figkit's own SVG rasterised
  with resvg, fonts supplied by the caller (the computational crates stay free
  of filesystem and font access). Experimental: its interface (options,
  pixmap vs PNG) may still change.
