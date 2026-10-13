# Browser adapter

`renderChart(dispatch, spec, options)` accepts a JavaScript specification
object or its original JSON source string. It uses `spec.layout.width` by
default; `options.width` overrides it. The host measures SVG text in the
loaded document font context and sends those dimensions to the Rust engine.

Use the **raw JSON string** form for exact decimal input. JavaScript objects
store numeric values as binary floating point, so distinct long decimal
literals may already have become the same number before `renderChart` runs.
The raw source is checked as JSON and spliced into the protocol request
without serializing its numeric tokens again. Text needing multiline SVG
layout is rejected explicitly.

## WebAssembly build and end-to-end test

```sh
cargo build -p chartgen-web --target wasm32-unknown-unknown
# the CLI version must equal wasm-bindgen's version in Cargo.lock (0.2.129)
cargo install wasm-bindgen-cli --version 0.2.129 --locked --root TOOLS
TOOLS/bin/wasm-bindgen --target nodejs --out-dir OUT target/wasm32-unknown-unknown/debug/chartgen_web.wasm
CHARTGEN_WEB_GLUE=OUT/chartgen_web.js node --test packages/web/wasm.test.mjs
```

`wasm.test.mjs` instantiates the real module, renders `examples/basic.json`
through its measurement rounds, checks that 28-digit decimals in raw JSON
source reach the engine exactly (two values differing in the 28th decimal sort
apart; as JavaScript numbers they tie), and that malformed protocol input
returns typed `protocol` errors. `python/tests/test_web_wasm.py` runs the same
steps. Text metrics there come from a deterministic stand-in for the browser's
SVG text measurement, not from browser fonts.
