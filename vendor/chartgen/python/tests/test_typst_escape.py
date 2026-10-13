"""chartgen.typst.escape is the Rust implementation, and equals the original figkit.typst.escape
on tests/conformance/figkit/fixtures.json (92 strings generated from the original)."""
import json
from pathlib import Path

import chartgen
from chartgen import typst

FIXTURES = json.loads((Path(__file__).resolve().parents[2] / "tests" / "conformance" / "figkit" /
                       "fixtures.json").read_text(encoding="utf-8"))


def test_escape_equals_the_original_and_calls_the_engine(monkeypatch):
    calls = []
    real = chartgen._native.dispatch
    monkeypatch.setattr(chartgen._native, "dispatch", lambda s: calls.append(s) or real(s))
    for case in FIXTURES["escape"]:
        assert typst.escape(*case["args"]) == case["expect"], case
    assert len(calls) == len(FIXTURES["escape"]) == 92
    assert typst.markup(None) is None and typst.markup("a_b") == "a\\_b"
