# Standalone figkit

Run `python packaging/figkit/stage.py /tmp/figkit-source` from this checkout,
then `cd /tmp/figkit-source && maturin build --release`. The staged distribution
contains only figure Rust crates, the independent Python package and manifests;
it contains no chartgen package, chart crate or chart style assets.

Install the resulting `figkit` wheel before installing chartgen. Chartgen declares
`figkit==0.1.0` and packages only its own namespace. `chartgen.figkit` submodules
are aliases of the standalone modules, including their classes and private
number-formatting helpers. The standalone native extension has no fallback.

The legacy `figkit.model`/`emit.svg` interface emits compatibility scene version 1.
`figkit.session` exposes version 2 transport and host registry management. Metrics
are supplied by the host; no portable operation estimates missing measurements.
`figkit.host` supplies optional legacy Typst process/cache conveniences. Native
keys denote source only in the legacy adapter; portable registries retain actual
host content under opaque keys until composition has finished.
