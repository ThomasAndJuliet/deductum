// End-to-end test of the browser build: the real chartgen_web WebAssembly module
// (wasm-bindgen glue for Node, path in CHARTGEN_WEB_GLUE) driven by renderChart.
// Text is measured by a deterministic stand-in for the browser's SVG text metrics.
import {test} from "node:test";
import assert from "node:assert/strict";
import {createRequire} from "node:module";
import {readFileSync} from "node:fs";
import {renderChart} from "./chartgen.mjs";

const glue = process.env.CHARTGEN_WEB_GLUE;
if (!glue) throw new Error("Set CHARTGEN_WEB_GLUE to the wasm-bindgen --target nodejs output (chartgen_web.js)");
const wasm = createRequire(import.meta.url)(glue);
const basic = readFileSync(new URL("../../examples/basic.json", import.meta.url), "utf8");

const measured = [];
globalThis.document = {
  fonts: {ready: Promise.resolve(), status: "loaded"},
  body: {appendChild() {}},
  createElementNS() {
    const attributes = {};
    return {
      namespaceURI: "http://www.w3.org/2000/svg", style: {}, textContent: "",
      setAttribute(name, value) { attributes[name] = value; }, appendChild() {}, remove() {},
      getBBox() { measured.push(this.textContent); const s = Number(attributes["font-size"]); return {height: 1.2 * s, y: -0.9 * s}; },
      getComputedTextLength() { return this.textContent.length * 0.5 * Number(attributes["font-size"]); },
    };
  },
};

function texts(svg) {
  return [...svg.matchAll(/<text x="([-\d.]+)" y="([-\d.]+)"[^>]*>([^<]*)<\/text>/g)]
    .map(([, x, y, text]) => ({x: Number(x), y: Number(y), text}));
}

test("the WebAssembly module renders a raw spec through its measurement rounds", async () => {
  measured.length = 0;
  let rounds = 0;
  const result = await renderChart(input => { ++rounds; return wasm.dispatch(input); }, basic);
  assert.equal(result.status, "complete");
  assert.ok(rounds >= 2, "at least one measurement round");
  assert.ok(measured.includes("Headcount by region"));
  assert.equal(result.report.type, "bar");
  assert.ok(result.output.startsWith("<svg"));
  const labels = texts(result.output).map(t => t.text);
  for (const name of ["Headcount by region", "East", "Central", "West"]) assert.ok(labels.includes(name), name);
  assert.equal((result.output.match(/<rect /g) || []).length >= 4, true);
});

test("28-digit decimals reach the engine exactly from raw JSON source", async () => {
  // East and Central differ only in the 28th decimal; the chart sorts bars by value.
  const raw = basic.replace("[48, 41, 29]", "[0.1234567890123456789012345678, 0.1234567890123456789012345679, 0.1]");
  assert.notEqual(raw, basic);
  const exact = await renderChart(wasm.dispatch, raw);
  const x = Object.fromEntries(texts(exact.output).map(t => [t.text, t.x]));
  assert.ok(x.Central < x.East, "the larger decimal sorts first");
  // A JavaScript object has already rounded both to the same double: they tie.
  assert.equal(JSON.parse(raw).data.series[0].values[0], JSON.parse(raw).data.series[0].values[1]);
  const rounded = await renderChart(wasm.dispatch, JSON.parse(raw));
  const y = Object.fromEntries(texts(rounded.output).map(t => [t.text, t.x]));
  assert.ok(y.East < y.Central, "tied doubles keep the given order");
});

test("malformed protocol input returns typed errors from the module", async () => {
  for (const [input, message] of [
    ["not json", "request is not JSON"],
    ['{"version":1,"operation":"draw"}', "unknown operation"],
    ['{"version":2,"operation":"schema"}', "unsupported protocol version 2"],
    ['{"version":1,"operation":"render","request":{"spec":{},"measurments":{}}}', "unknown field(s): measurments"],
  ]) {
    const response = JSON.parse(wasm.dispatch(input));
    assert.equal(response.version, 1);
    assert.equal(response.outcome, "error");
    assert.equal(response.error.kind, "protocol");
    assert.ok(response.error.message.includes(message), response.error.message);
  }
  await assert.rejects(renderChart(() => wasm.dispatch("{"), basic), /protocol: request is not JSON/);
  await assert.rejects(renderChart(wasm.dispatch, "[1]"), /must be a JSON object/);
});

test("library marks (action dots, paper ISSUES #2) render in the browser build", async () => {
  const file = JSON.parse(readFileSync(new URL("../../tests/conformance/examples/specs/p3-54-action-dots.json", import.meta.url), "utf8"));
  const spec = file.base ?? file;
  const result = await renderChart(wasm.dispatch, JSON.stringify(spec));
  assert.equal(result.status, "complete", JSON.stringify(result.report?.findings));
  assert.ok((result.output.match(/<circle /g) || []).length > 0);
  assert.ok(!result.output.includes("<use"));
});
