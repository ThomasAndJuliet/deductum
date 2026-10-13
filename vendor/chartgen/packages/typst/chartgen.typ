// Chartgen document adapter. All numeric plan coordinates are physical pt (1/72 inch).
// The plugin is byte-only; native Typst content stays in this wrapper.
// Send protocol input (bytes) to the plugin and validate the response.
#let chartgen-dispatch-bytes(module, input) = {
  let dispatch = module.dispatch
  let response = json(dispatch(input))
  // Malformed responses fail here, explicitly, rather than at a later field access.
  assert(type(response) == dictionary and response.at("version", default: none) == 1,
    message: "chartgen: malformed protocol response (expected a version 1 object)")
  let field = (render: "result", output: "output", data: "value", error: "error")
    .at(response.at("outcome", default: ""), default: none)
  assert(field != none and field in response,
    message: "chartgen: malformed protocol response (unknown outcome or missing field)")
  if response.outcome == "error" {
    panic("chartgen " + response.error.at("kind", default: "?") + " error: "
      + response.error.at("message", default: ""))
  }
  response
}

#let chartgen-dispatch(module, request) = chartgen-dispatch-bytes(module, bytes(json.encode(request)))

#let chartgen-content(content, native) = {
  if content.kind == "plain" {
    content.text
  } else if content.kind == "native" and content.host == "typst" {
    if content.key in native {
      native.at(content.key)
    } else {
      eval(content.key, mode: "markup")
    }
  } else if content.kind == "native" and content.alternative != none {
    content.alternative
  } else {
    panic("unsupported chartgen text content")
  }
}

// Text is set as the original measures and bakes it: its box is its ink
// (top and bottom edges at the glyph bounds), its advance the box width.
#let chartgen-styled(body, request, bottom: "bounds") = {
  let styling = request.at("style", default: (:))
  let weight = if styling.at("weight", default: "normal") == "normal" { "regular" } else { styling.weight }
  let slant = styling.at("font_style", default: "normal")
  let ink = styling.at("fill", default: none)
  set text(top-edge: "bounds", bottom-edge: bottom)
  if ink == none {
    text(font: request.font_family, size: request.font_size * 1pt,
      weight: weight, style: slant, body)
  } else {
    text(font: request.font_family, size: request.font_size * 1pt,
      weight: weight, style: slant, fill: rgb(ink), body)
  }
}

// Measure each TextRequest in the same document context used to draw the chart:
// width is the advance, height the ink box, baseline the ink ascent. A label
// wider than its max_width is broken within it (widest line, no single baseline).
// The result can be serialized with json.encode and sent to the plugin's next step.
#let chartgen-measure(requests, native: (:)) = {
  let entries = (:)
  for request in requests {
    let body = chartgen-content(request.content, native)
    let line = measure(chartgen-styled(body, request))
    let ascent = measure(chartgen-styled(body, request, bottom: "baseline")).height
    entries.insert(request.id, if request.max_width == none or line.width <= request.max_width * 1pt {
      (width: line.width / 1pt, height: line.height / 1pt, baseline: ascent / 1pt)
    } else {
      let dims = measure(chartgen-styled(body, request), width: request.max_width * 1pt)
      (width: dims.width / 1pt, height: dims.height / 1pt, baseline: none)
    })
  }
  entries
}

#let chartgen-compose(plan, native: (:)) = {
  assert(plan.version == 1, message: "unsupported chartgen Typst plan version")
  let canvas = []
  for layer in plan.layers {
    if layer.kind == "svg" {
      canvas += place(top + left, dx: 0pt, dy: 0pt,
        image(bytes(layer.svg), format: "svg", width: plan.width * 1pt, height: plan.height * 1pt))
    } else if layer.kind == "text" {
      let body = chartgen-content(layer.content, native)
      body = chartgen-styled(body, layer)
      let label = box(width: layer.width * 1pt, height: layer.height * 1pt, inset: 0pt, body)
      if layer.clip == none {
        canvas += place(top + left, dx: layer.x * 1pt, dy: layer.y * 1pt, label)
      } else {
        let c = layer.clip
        let clipped = box(width: c.width * 1pt, height: c.height * 1pt,
          clip: true, inset: 0pt,
          place(top + left, dx: (layer.x - c.x) * 1pt, dy: (layer.y - c.y) * 1pt, label))
        canvas += place(top + left, dx: c.x * 1pt, dy: c.y * 1pt, clipped)
      }
    } else {
      panic("unsupported chartgen Typst layer")
    }
  }
  box(width: plan.width * 1pt, height: plan.height * 1pt, clip: true, inset: 0pt, canvas)
}

// Complete protocol loop for the browser-independent Rust plugin dispatcher.
// A caller may supply actual Typst content in `native`; otherwise native keys
// are evaluated as explicit Typst markup supplied by the chart specification.
// `spec` is a dictionary, or the specification's raw JSON source as a string
// (for example `read("chart.json")`). Typst parses JSON numbers as 64-bit floats;
// only the raw source carries long decimals to the engine unchanged, because it
// is spliced into the request without being decoded and encoded again.
#let chart(spec, module, theme: none, constraints: none, native: (:), variant: none, expect: (:)) = context {
  let raw = if type(spec) == str { spec } else { none }
  let parsed = if raw == none { spec } else { json(bytes(raw)) }
  assert(type(parsed) == dictionary, message: "chartgen: a chart specification must be a JSON object")
  let theme = if theme == none {
    (name: "document", font_family: if type(text.font) == str { text.font } else { text.font.first() },
     font_size: text.size / 1pt, text: "#595959", grid: "#dddddd", background: "#ffffff",
     palette: ("#527a8a", "#a37753", "#71865d", "#89718b", "#ae706f"), tokens: (:))
  } else { theme }
  // The original's default figure width is 368 pt (the paper's text width).
  let layout = parsed.at("layout", default: (:))
  let size = if constraints == none { (width: layout.at("width", default: 368.0), height: layout.at("height", default: none), measurement_context: "typst", max_rounds: 8) } else { constraints }
  let measurements = ("context": size.measurement_context, entries: (:))
  let render(measurements) = {
    let request = (theme: theme, constraints: size,
      measurements: measurements, backend: "typst", variant: variant, expect: expect)
    if raw == none {
      chartgen-dispatch(module, (version: 1, operation: "render", request: (spec: spec) + request))
    } else {
      let rest = json.encode(request, pretty: false)
      chartgen-dispatch-bytes(module, bytes("{\"version\":1,\"operation\":\"render\",\"request\":{\"spec\":"
        + raw + "," + rest.slice(1) + "}"))
    }
  }
  let response = render(measurements)
  let rounds = 0
  while response.outcome == "render" and response.result.status == "need_measurements" {
    assert(rounds < size.max_rounds, message: "chartgen measurement limit exceeded")
    // Entries accumulate: a later round asks only for text it has not seen.
    for (id, metrics) in chartgen-measure(response.result.requests, native: native) {
      measurements.entries.insert(id, metrics)
    }
    response = render(measurements)
    rounds += 1
  }
  assert(response.outcome == "render", message: "chartgen: a render request must return a render outcome")
  if response.result.status != "complete" {
    panic("chartgen stopped: " + response.result.status)
  }
  let plan = json(bytes(response.result.output))
  // The plan stays queryable (typst query DOC "<chartgen-plan>") to check placement.
  [#metadata(plan) <chartgen-plan>]
  chartgen-compose(plan, native: native)
}
