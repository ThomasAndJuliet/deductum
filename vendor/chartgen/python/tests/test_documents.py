"""Native document text stays host-owned, searchable and where the layout put it.

A chart with source-provided native markup (Typst markup; TeX content declared
in the host preamble) is compiled into a PDF by the real document engine (the
Typst CLI with the chartgen_typst WebAssembly plugin; pdflatex with
packages/latex). The PDF's extracted text, fonts and word positions are then
checked against the specification and the engine's placement.

The tools are required: a missing tool fails these tests. Set
CHARTGEN_ALLOW_MISSING_TOOLS=1 to turn that failure into an explicit skip.
"""
import copy
import json
import os
import re
import shutil
import subprocess
from pathlib import Path

import pytest

import chartgen
from chartgen.latex import TeXMeasurer

from conftest import require

ROOT = Path(__file__).resolve().parents[2]
BASIC = json.loads((ROOT / "examples" / "basic.json").read_text())
PLUGIN = ROOT / "target" / "wasm32-unknown-unknown" / "debug" / "chartgen_typst.wasm"


def words(pdf):
    """[(word, xMin, yMin, xMax, yMax)] in PDF points from the top-left, by pdftotext."""
    html = subprocess.run(["pdftotext", "-bbox", str(pdf), "-"], capture_output=True, text=True, check=True).stdout
    return [(w, float(a), float(b), float(c), float(d)) for a, b, c, d, w in re.findall(
        r'<word xMin="([\d.]+)" yMin="([\d.]+)" xMax="([\d.]+)" yMax="([\d.]+)">([^<]*)</word>', html)]


def fonts(pdf):
    return subprocess.run(["pdffonts", str(pdf)], capture_output=True, text=True, check=True).stdout


def text(pdf):
    return " ".join(subprocess.run(["pdftotext", str(pdf), "-"], capture_output=True, text=True,
                                   check=True).stdout.split())


def find(found, word):
    hits = [w for w in found if w[0] == word]
    assert len(hits) == 1, f"{word!r} occurs {len(hits)} times in {[w[0] for w in found]}"
    return hits[0]


def test_typst_document_keeps_native_markup_and_places_it_by_the_plan(tmp_path):
    require("typst", "the Typst CLI")
    require("pdftotext", "poppler-utils")
    require(str(PLUGIN), "cargo build -p chartgen-typst --target wasm32-unknown-unknown")
    spec = copy.deepcopy(BASIC)
    spec["title"] = {"text": {"typst": "Headcount by _region_ #strong[in 2026]"}}
    spec["data"]["x"]["names"] = {"West": {"typst": "west"}}
    (tmp_path / "spec.json").write_text(json.dumps(spec))
    doc = tmp_path / "doc.typ"
    doc.write_text(
        f'#import "{ROOT}/packages/typst/chartgen.typ": chart\n'
        f'#let engine = plugin("{PLUGIN}")\n'
        '#set page(width: 520pt, height: auto, margin: 20pt)\n'
        '#set text(font: "Libertinus Serif", size: 10pt)\n'
        f'#chart(json("{tmp_path}/spec.json"), engine, native: (west: [#text(style: "italic")[West]]))\n')
    pdf = tmp_path / "doc.pdf"
    run = subprocess.run(["typst", "compile", "--root", "/", str(doc), str(pdf)], capture_output=True, text=True)
    assert run.returncode == 0, run.stderr
    assert "Headcount by region in 2026" in text(pdf)
    used = fonts(pdf)
    assert "LibertinusSerif-Italic" in used and "LibertinusSerif-Bold" in used and "LibertinusSerif-Regular" in used
    plan = json.loads(subprocess.run(["typst", "query", "--root", "/", str(doc), "<chartgen-plan>", "--field", "value",
                                      "--one"], capture_output=True, text=True, check=True).stdout)
    layers = [l for l in plan["layers"] if l["kind"] == "text"]
    assert {l["content"]["kind"] for l in layers} == {"plain", "native"}
    found = words(pdf)
    # Each text layer's box (top-left x, y in pt) sits at the page margin plus its plan position.
    for layer in layers:
        content = layer["content"]
        first = {"west": "West"}.get(content.get("key"), (content.get("text") or content.get("key") or "").split()[0])
        if content["kind"] == "native" and content["key"].startswith("Headcount"):
            first = "Headcount"
        w = find(found, first)
        assert abs(w[1] - (20 + layer["x"])) < 1.0, (layer, w)
        assert abs(w[2] - (20 + layer["y"])) < 3.0, (layer, w)


def test_pgf_document_sets_declared_tex_content_with_the_host_preamble(tmp_path):
    for tool in ("pdflatex", "pdftotext", "pdffonts"):
        require(tool, "TeX Live and poppler-utils")
    preamble = r"\usepackage[T1]{fontenc}\usepackage{lmodern}"
    native = {"title": r"Headcount by \textit{region}", "west": r"\textbf{West}"}
    spec = copy.deepcopy(BASIC)
    spec["title"] = {"text": {"latex": "title", "alternative": "Headcount by region"}}
    spec["data"]["x"]["names"] = {"West": {"latex": "west", "alternative": "West"}}
    measurer = TeXMeasurer(preamble, native)
    pgf, report = chartgen.render(spec, "plain", backend="pgf", measurer=measurer)
    assert report.status == "drawn"
    assert r"\chartgenUseNative{title}" in pgf and r"\chartgenUseNative{west}" in pgf
    # The same layout drawn as SVG gives the engine's baseline positions.
    svg, _ = chartgen.render(spec, "plain", backend="svg", measurer=measurer)
    placed = {t: (float(x), float(y)) for x, y, t in re.findall(r'<text x="([-\d.]+)" y="([-\d.]+)"[^>]*>([^<]+)</text>', svg)}
    (tmp_path / "chart.pgf").write_text(pgf)
    (tmp_path / "doc.tex").write_text(
        "\\documentclass{article}\n" + preamble + "\n\\usepackage{chartgen}\n\\pagestyle{empty}\n"
        "\\begin{document}\n" + measurer.declarations() + "\n\\noindent\\input{chart.pgf}\n\\end{document}\n")
    env = {**os.environ, "TEXINPUTS": f"{ROOT / 'packages' / 'latex'}:"}
    run = subprocess.run(["pdflatex", "-interaction=nonstopmode", "-halt-on-error", "doc.tex"], cwd=tmp_path,
                         capture_output=True, text=True, env=env)
    assert run.returncode == 0, run.stdout[-2000:]
    pdf = tmp_path / "doc.pdf"
    assert "Headcount by region" in text(pdf)
    used = fonts(pdf)
    # The bold title role with the declared \textit{region}, and the declared \textbf{West}.
    assert re.search(r"LMRoman\d+-BoldItalic", used) and re.search(r"LMRoman\d+-Bold\s", used), used
    found = words(pdf)
    # Words keep the engine's relative placement (TeX bp = 1/72 in; pdftotext reports the same unit).
    pairs = [(find(found, word), placed[label]) for word, label in
             (("Headcount", "Headcount by region"), ("East", "East"), ("Central", "Central"), ("West", "West"))]
    dx = [w[1] - x for w, (x, _) in pairs]
    dy = [w[4] - y for w, (_, y) in pairs]
    assert max(dx) - min(dx) < 1.0, pairs
    assert max(dy) - min(dy) < 1.5, pairs


def test_native_tex_title_wider_than_the_chart_wraps_in_the_document(tmp_path):
    # paper/ISSUES.md #10: a native TeX title wider than its width wraps as a plain one does.
    for tool in ("pdflatex", "pdftotext"):
        require(tool, "TeX Live and poppler-utils")
    long = (r"Nitrate at the \textit{harbour} station stayed above the permit for every month of the "
            r"monitoring year")
    native = {"title": long}
    spec = copy.deepcopy(BASIC)
    spec["layout"] = {"width": 300}
    spec["title"] = {"text": {"latex": "title", "alternative": "Nitrate at the harbour station"}}
    measurer = TeXMeasurer("", native)
    pgf, report = chartgen.render(spec, "plain", backend="pgf", measurer=measurer)
    assert report.status == "drawn"
    (tmp_path / "chart.pgf").write_text(pgf)
    (tmp_path / "doc.tex").write_text(
        "\\documentclass{article}\n\\usepackage{chartgen}\n\\pagestyle{empty}\n\\begin{document}\n"
        + measurer.declarations() + "\n\\noindent\\input{chart.pgf}\n\\end{document}\n")
    env = {**os.environ, "TEXINPUTS": f"{ROOT / 'packages' / 'latex'}:"}
    run = subprocess.run(["pdflatex", "-interaction=nonstopmode", "-halt-on-error", "doc.tex"], cwd=tmp_path,
                         capture_output=True, text=True, env=env)
    assert run.returncode == 0, run.stdout[-2000:]
    pdf = tmp_path / "doc.pdf"
    assert "monitoring year" in text(pdf)
    found = words(pdf)
    title = [w for w in found if w[0] in ("Nitrate", "harbour", "monitoring", "year")]
    assert len({round(w[4]) for w in title}) >= 2, title          # on more than one line
    left = find(found, "Nitrate")[1]
    assert max(w[3] for w in title) - left <= 300 + 0.5            # within the chart's width
    assert max(w[4] for w in title) < find(found, "East")[2]       # above the plot's labels
