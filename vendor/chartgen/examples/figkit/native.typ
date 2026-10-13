#import "../../packages/figkit-typst/figkit.typ": measure, render
#set page(width: 280pt, height: 110pt, margin: 20pt)
#let module = plugin(sys.inputs.at("wasm", default: "../../target/wasm32-unknown-unknown/debug/figkit_typst.wasm"))
#let c = (id: "example", engine: "typst", engine_version: "local", font_environment: "New Computer Modern", settings: "", registry_revision: "1")
#let r = (id: "equation", content: (kind: "native", host: "typst", key: "equation", alternative: none), typography: (font_family: "New Computer Modern", font_size: 12, weight: "normal", font_style: "normal"), max_width: none, "context": c)
#let registry = (equation: [Native #strong[mathematics]: $x^2+y^2=r^2$])
#context {
  let state = measure(module, (r,), registry, c)
  let figure = (version: 2, size: (width: 240, height: 70), background: "white",
    metadata: (title: "Native Typst figure", description: none), resources: (clips: (:), symbols: (:)),
    primitives: ((kind: "text", placed: (
      text: (request: r, metrics: state.replies.equation.metrics, paint: (kind: "resolved", fill: "black")),
      position: (x: 12, y: 25), anchor: "start",
      metadata: (element_id: none, authored_id: none, classes: (), label_group: none))),))
  render(module, figure, registry, c)
}
