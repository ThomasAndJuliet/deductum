import {test} from "node:test";
import assert from "node:assert/strict";
import {renderChart} from "./chartgen.mjs";

const fontReady = Promise.resolve();
globalThis.document = {
  fonts: {ready: fontReady, status: "loaded"},
  body: {appendChild() {}},
  createElementNS() {
    return {
      namespaceURI: "http://www.w3.org/2000/svg", style: {}, textContent: "",
      setAttribute() {}, appendChild() {}, remove() {},
      getBBox() { return {height: 10, y: -8}; },
      getComputedTextLength() { return this.textContent.length * 5; },
    };
  },
};

const complete = JSON.stringify({version: 1, outcome: "render", result: {status: "complete", output: "ok"}});
test("raw JSON preserves decimal lexemes and spec layout width", async () => {
  let input;
  const raw = '{"layout":{"width":321},"data":{"values":[0.1234567890123456789012345678,0.1234567890123456789012345679]}}';
  await renderChart(value => {input = value; return complete;}, raw);
  assert.ok(input.includes("0.1234567890123456789012345678,0.1234567890123456789012345679"));
  assert.equal(JSON.parse(input).request.constraints.width, 321);
});

test("object specs use layout width, and one measurement round can complete", async () => {
  const inputs = [];
  const result = await renderChart(input => {
    inputs.push(JSON.parse(input));
    if (inputs.length === 1) return JSON.stringify({version: 1, outcome: "render", result: {
      status: "need_measurements", requests: [{id: "a", content: {kind: "plain", text: "Hello"}, font_family: "serif", font_size: 10, max_width: null}], round: 1,
    }});
    return complete;
  }, {layout: {width: 275}}, {maxRounds: 1});
  assert.equal(result.status, "complete");
  assert.equal(inputs.length, 2);
  assert.equal(inputs[1].request.measurements.entries.a.width, 25);
  assert.equal(inputs[1].request.constraints.width, 275);
});

test("browser refuses multiline SVG labels", async () => {
  await assert.rejects(renderChart(() => JSON.stringify({version: 1, outcome: "render", result: {
    status: "need_measurements", requests: [{id: "a", content: {kind: "plain", text: "A\nB"}, font_family: "serif", font_size: 10, max_width: null}], round: 1,
  }}), {}), /Multiline SVG text/);
});

test("browser rejects unknown protocol outcomes and render statuses", async () => {
  await assert.rejects(renderChart(() => JSON.stringify({version: 1, outcome: "data", value: []}), {}), /Unexpected chartgen render response/);
  await assert.rejects(renderChart(() => JSON.stringify({version: 1, outcome: "render", result: {status: "skipped"}}), {}), /Unexpected chartgen render status: skipped/);
});

test("browser measures italic and bold requests with their actual font attributes", async () => {
  const seen = [];
  const create = document.createElementNS;
  document.createElementNS = () => ({namespaceURI: "http://www.w3.org/2000/svg", style: {},
    setAttribute(name, value) {seen.push([name, value]);}, appendChild() {}, remove() {},
    getBBox() {return {height: 10, y: -8};}, getComputedTextLength() {return 20;}});
  try {
    let count = 0;
    await renderChart(() => ++count === 1 ? JSON.stringify({version: 1, outcome: "render", result: {
      status: "need_measurements", requests: [
        {id: "italic", content: {kind: "plain", text: "A"}, font_family: "serif", font_size: 10,
         style: {fill: "#3f63b0", weight: "normal", font_style: "italic"}, max_width: null},
        {id: "bold", content: {kind: "plain", text: "B"}, font_family: "serif", font_size: 10,
         style: {fill: "#595959", weight: "bold", font_style: "normal"}, max_width: null},
      ]}}) : complete, {});
    assert.ok(seen.some(([name, value]) => name === "font-style" && value === "italic"));
    assert.ok(seen.some(([name, value]) => name === "font-weight" && value === "bold"));
    assert.ok(seen.some(([name, value]) => name === "fill" && value === "#3f63b0"));
  } finally {document.createElementNS = create;}
});
