# Browser peer

Build `figkit-web` with wasm-bindgen for the browser or Node target, then pass
its exported `dispatch` to `Session`. Register host DOM content before measurement.
`measure(requests, measureBatch)` expects replies retaining each complete request
and `{size:{width,height},baseline}` metrics in points. The callback owns actual
font measurement. `render(figure, 'browser')` returns a JSON placement plan;
`compose(JSON.parse(plan), session)` creates ordered geometry/text DOM layers.
Close the session only after composition. No Python or chartgen package is used.
