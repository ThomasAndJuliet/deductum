// Independent plugin transport. Native keys index host-owned content, never source code.
#let call(plugin, operation, ..fields) = {
  let reply = json(plugin.dispatch(bytes(json.encode((version: 2, operation: operation) + fields.named()))))
  assert(reply.outcome != "error", message: repr(reply.at("error", default: none)))
  reply.at("value", default: reply.at("output", default: none))
}
#let measure-batch(requests, registry) = {
  requests.map(r => {
    assert(r.content.kind != "native" or r.content.host == "typst", message: "unsupported native host for Typst")
    let content = if r.content.kind == "native" { registry.at(r.content.key) } else { r.content.text }
    let t = r.typography
    let block = text(font: t.font_family, size: t.font_size * 1pt, weight: if t.weight == "normal" { "regular" } else { t.weight }, style: t.font_style, top-edge: "bounds", bottom-edge: "bounds", content)
    let dimensions = measure(block, width: if r.max_width == none { auto } else { r.max_width * 1pt })
    (request: r, metrics: (size: (width: dimensions.width / 1pt, height: dimensions.height / 1pt), baseline: none))
  })
}
// Match the portable core's solid-colour domain, including normalized CSS names.
#let resolved-paint(value) = {
  let value = lower(value.trim())
  let named = (black: "#000000", white: "#ffffff", red: "#ff0000",
    green: "#008000", blue: "#0000ff", gray: "#808080", grey: "#808080")
  rgb(named.at(value, default: value))
}
#let compose(plan, registry, measurement-context) = {
  assert(plan.version == 2, message: "unsupported figkit plan")
  let canvas = []
  for layer in plan.layers {
    if layer.kind == "svg" {
      canvas += place(top + left, image(bytes(layer.svg), format: "svg", width: plan.width * 1pt, height: plan.height * 1pt))
    } else if layer.kind == "text" {
      let r = layer.request
      assert(r.context == measurement-context, message: "stale figkit composition context")
      let body = if r.content.kind == "native" { registry.at(r.content.key) } else { r.content.text }
      let t = r.typography
      let styling = (font: t.font_family, size: t.font_size * 1pt,
        weight: if t.weight == "normal" { "regular" } else { t.weight }, style: t.font_style,
        top-edge: "bounds", bottom-edge: "bounds")
      if layer.paint.kind == "resolved" { styling.insert("fill", resolved-paint(layer.paint.fill)) }
      body = text(..styling, body)
      let label = box(width: layer.metrics.size.width * 1pt, height: layer.metrics.size.height * 1pt, inset: 0pt, body)
      if layer.clip == none {
        canvas += place(top + left, dx: layer.x * 1pt, dy: layer.y * 1pt, label)
      } else {
        let c = layer.clip
        let clipped = box(width: c.width * 1pt, height: c.height * 1pt, clip: true, inset: 0pt,
          place(top + left, dx: (layer.x-c.x) * 1pt, dy: (layer.y-c.y) * 1pt, label))
        canvas += place(top + left, dx: c.x * 1pt, dy: c.y * 1pt, clipped)
      }
    } else { panic("unknown figkit layer") }
  }
  box(width: plan.width * 1pt, height: plan.height * 1pt, inset: 0pt, clip: true, canvas)
}
#let measure(module, requests, registry, measurement-context, session: none) = {
  let result = call(module, "measure", ..("context": measurement-context, session: session, requests: requests, replies: ()))
  while not result.complete {
    let replies = measure-batch(result.requests, registry)
    assert(replies.len() > 0, message: "measurement made no progress")
    result = call(module, "measure", session: result.session, requests: requests, replies: replies)
  }
  result.session
}
#let render(module, figure, registry, measurement-context) = {
  let plan = json(bytes(call(module, "render_scene", figure: figure, backend: "typst")))
  compose(plan, registry, measurement-context)
}
