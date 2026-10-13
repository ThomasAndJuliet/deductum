// Browser host: SVG measurements and a byte-preserving raw JSON specification path.
// JavaScript numbers are IEEE-754 doubles. Pass the original JSON source as a
// string when decimal digits beyond that precision must reach Rust unchanged.
export async function renderChart(dispatch, spec, {theme, width, height, backend = "svg", maxRounds = 8} = {}) {
  await document.fonts.ready;
  // Without a theme the engine uses its default, the original plain style with its stylesheet
  // (label typography comes from the stylesheet's classes); an undefined theme is not sent.
  // Parsing a raw source here reads only layout metadata. The parsed copy is
  // never serialized or sent; `specJson` carries the original decimal lexemes.
  const parsed = typeof spec === "string" ? JSON.parse(spec) : spec;
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new TypeError("Chart specification must be a JSON object or its raw JSON source");
  }
  const specJson = typeof spec === "string" ? spec : JSON.stringify(spec);
  // The original's default figure width is 368 pt (the paper's text width).
  const chartWidth = width ?? parsed.layout?.width ?? 368;
  height ??= parsed.layout?.height ?? null;
  const context = JSON.stringify({theme, fonts: document.fonts.status, width:chartWidth, height});
  const request = {theme, backend, constraints:{width:chartWidth,height,measurement_context:context,max_rounds:maxRounds},measurements:{context,entries:{}}};
  for (let round = 0; round <= maxRounds; ++round) {
    // Only the surrounding protocol is stringified. A raw spec is spliced in
    // unchanged after its JSON syntax has been checked above.
    const requestJson = JSON.stringify(request);
    const input = `{"version":1,"operation":"render","request":{"spec":${specJson},${requestJson.slice(1)}}`;
    const response = JSON.parse(dispatch(input));
    if (response.outcome === "error") throw new Error(`${response.error.kind}: ${response.error.message}`);
    if (response.version !== 1 || response.outcome !== "render" || !response.result) {
      throw new Error("Unexpected chartgen render response");
    }
    const result = response.result;
    if (result.status === "complete" || result.status === "stopped") return result;
    if (result.status !== "need_measurements") {
      throw new Error(`Unexpected chartgen render status: ${result.status}`);
    }
    if (round === maxRounds) break;
    const node = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    node.style.cssText = "position:absolute;visibility:hidden;overflow:visible;pointer-events:none";
    document.body.appendChild(node);
    try {
      for (const r of result.requests) {
        const text = document.createElementNS(node.namespaceURI, "text");
        const c = r.content;
        if (c.kind !== "plain" && c.alternative == null) throw new Error("Native document label needs a web text alternative");
        const value = c.kind === "plain" ? c.text : c.alternative;
        if (/[\r\n]/.test(value)) throw new Error("Multiline SVG text needs a measured line layout");
        text.textContent = value;
        text.setAttribute("font-family",r.font_family);
        text.setAttribute("font-size",r.font_size);
        text.setAttribute("font-weight",r.style?.weight ?? "normal");
        text.setAttribute("font-style",r.style?.font_style ?? "normal");
        if (r.style?.fill != null) text.setAttribute("fill",r.style.fill);
        node.appendChild(text);
        const box = text.getBBox();
        if (r.max_width != null && text.getComputedTextLength() > r.max_width) throw new Error("Label needs wrapped SVG text, which is not yet supported by this browser host");
        request.measurements.entries[r.id] = {width:text.getComputedTextLength(),height:box.height,baseline:-box.y};
      }
    } finally { node.remove(); }
  }
  throw new Error("Chart layout exceeded the measurement round limit");
}
