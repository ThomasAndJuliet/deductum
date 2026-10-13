#import "chartgen.typ": chart
#let layout-width = 321.0
#let plan = (version: 1, width: layout-width, height: 30.0,
  layers: ((kind: "text", x: 5.0, y: 5.0, width: 40.0, height: 12.0,
    font_family: "Libertinus Serif", font_size: 10.0,
    content: (kind: "plain", text: "Width"), clip: none),))
#let dispatch(arg) = {
  let req = json(arg)
  assert(req.request.constraints.width == layout-width, message: "spec.layout.width was ignored")
  let result = if req.request.measurements.entries.len() == 0 {
    (status: "need_measurements", round: 1,
      requests: ((id: "foreign", role: "label", font_family: "Libertinus Serif",
        font_size: 10.0, max_width: none,
        content: (kind: "native", host: "latex", key: "unavailable",
          alternative: "Readable fallback")),))
  } else {
    assert(req.request.measurements.entries.at("foreign").width > 0)
    (status: "complete", output: json.encode(plan))
  }
  bytes(json.encode((version: 1, outcome: "render", result: result)))
}
#set page(width: 400pt, height: 100pt)
#chart((layout: (width: layout-width)), (dispatch: dispatch))
