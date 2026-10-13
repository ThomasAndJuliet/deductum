"""The Typst wrapper (packages/typst/chartgen.typ) against scripted plugin responses:
measurement entries accumulate across rounds, and malformed protocol responses
stop compilation with an explicit message."""
import json
import subprocess
from pathlib import Path

import pytest

from conftest import require

ROOT = Path(__file__).resolve().parents[2]
PLAN = {"version": 1, "width": 100.0, "height": 30.0, "layers": []}


def compile_with(tmp_path, dispatch_body):
    require("typst", "the Typst CLI")
    doc = tmp_path / "doc.typ"
    doc.write_text(f'#import "{ROOT}/packages/typst/chartgen.typ": chart\n'
                   f"#let dispatch(arg) = {{\n  let req = json(arg)\n{dispatch_body}\n}}\n"
                   "#set page(width: 200pt, height: 100pt)\n"
                   "#chart((layout: (width: 100.0)), (dispatch: dispatch))\n")
    return subprocess.run(["typst", "compile", "--root", "/", str(doc), str(tmp_path / "doc.pdf")],
                          capture_output=True, text=True)


def request(ident, text):
    return {"id": ident, "role": "label", "font_family": "Libertinus Serif", "font_size": 10.0,
            "max_width": None, "content": {"kind": "plain", "text": text}}


def test_measurement_rounds_accumulate_entries(tmp_path):
    first, second = json.dumps(request("a", "Alpha")), json.dumps(request("b", "Beta"))
    complete = json.dumps({"status": "complete", "output": json.dumps(PLAN)})
    body = f"""  let entries = req.request.measurements.entries
  let result = if entries.len() == 0 {{
    (status: "need_measurements", round: 1, requests: (json(bytes({json.dumps(first)})),))
  }} else if entries.len() == 1 {{
    (status: "need_measurements", round: 2, requests: (json(bytes({json.dumps(second)})),))
  }} else {{
    assert("a" in entries and "b" in entries, message: "round one's entry was dropped")
    assert(entries.a.width > entries.b.width and entries.a.baseline > 0, message: "bad metrics")
    json(bytes({json.dumps(complete)}))
  }}
  bytes(json.encode((version: 1, outcome: "render", result: result)))"""
    run = compile_with(tmp_path, body)
    assert run.returncode == 0, run.stderr


@pytest.mark.parametrize("response, message", [
    ('(version: 2, outcome: "render", result: (:))', "malformed protocol response"),
    ('(version: 1, outcome: "render")', "malformed protocol response"),
    ('(version: 1, outcome: "surprise", value: 1)', "malformed protocol response"),
    ('(1, 2)', "malformed protocol response"),
    ('(version: 1, outcome: "data", value: ())', "must return a render outcome"),
    ('(version: 1, outcome: "error", error: (kind: "protocol", message: "unknown field(s): x"))',
     "chartgen protocol error: unknown field(s): x"),
    ('(version: 1, outcome: "render", result: (status: "stopped", report: (:)))', "chartgen stopped: stopped"),
])
def test_malformed_or_failed_responses_stop_compilation_explicitly(tmp_path, response, message):
    run = compile_with(tmp_path, f"  bytes(json.encode({response}))")
    assert run.returncode != 0
    assert message in run.stderr, run.stderr


PLUGIN = ROOT / "target" / "wasm32-unknown-unknown" / "debug" / "chartgen_typst.wasm"


def test_real_plugin_answers_malformed_input_with_typed_errors(tmp_path):
    require("typst", "the Typst CLI")
    require(str(PLUGIN), "cargo build -p chartgen-typst --target wasm32-unknown-unknown")
    doc = tmp_path / "doc.typ"
    doc.write_text(f'#let engine = plugin("{PLUGIN}")\n'
                   '#for (input, message) in (("not json", "request is not JSON"),\n'
                   '    ("{\\"version\\":1,\\"operation\\":\\"draw\\"}", "unknown operation"),\n'
                   '    ("{\\"version\\":1,\\"operation\\":\\"schema\\",\\"x\\":1}", "unknown field(s): x")) {\n'
                   '  let r = json(engine.dispatch(bytes(input)))\n'
                   '  assert(r.version == 1 and r.outcome == "error" and r.error.kind == "protocol")\n'
                   '  assert(r.error.message.contains(message), message: r.error.message)\n'
                   '}\nok\n')
    run = subprocess.run(["typst", "compile", "--root", "/", str(doc), str(tmp_path / "doc.pdf")],
                         capture_output=True, text=True)
    assert run.returncode == 0, run.stderr


def test_raw_spec_source_carries_28_digit_decimals_through_the_plugin(tmp_path):
    require("typst", "the Typst CLI")
    require(str(PLUGIN), "cargo build -p chartgen-typst --target wasm32-unknown-unknown")
    basic = (ROOT / "examples" / "basic.json").read_text()
    raw = basic.replace("[48, 41, 29]", "[0.1234567890123456789012345678, 0.1234567890123456789012345679, 0.1]")
    assert raw != basic
    (tmp_path / "spec.json").write_text(raw)

    def category_x(argument):
        doc = tmp_path / "doc.typ"
        doc.write_text(f'#import "{ROOT}/packages/typst/chartgen.typ": chart\n'
                       f'#let engine = plugin("{PLUGIN}")\n'
                       '#set page(width: 520pt, height: auto, margin: 20pt)\n'
                       f'#chart({argument}, engine)\n')
        plan = json.loads(subprocess.run(["typst", "query", "--root", "/", str(doc), "<chartgen-plan>",
                                          "--field", "value", "--one"], capture_output=True, text=True,
                                         check=True).stdout)
        return {l["content"].get("text"): l["x"] for l in plan["layers"] if l["kind"] == "text"}

    # East and Central differ in the 28th decimal; bars sort by value.
    exact = category_x(f'read("{tmp_path}/spec.json")')
    assert exact["Central"] < exact["East"]
    # Decoded by Typst, both are the same 64-bit float and keep the given order.
    rounded = category_x(f'json("{tmp_path}/spec.json")')
    assert rounded["East"] < rounded["Central"]
