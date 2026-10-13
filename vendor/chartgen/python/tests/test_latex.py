import json
from pathlib import Path
import shutil
import subprocess
import pytest
from chartgen import render
from chartgen.latex import TeXMeasurer
from conftest import require

def test_tex_native_metrics_drive_chart_and_pgf_compiles(tmp_path):
    require("pdflatex", "TeX Live")
    spec=json.loads((Path(__file__).resolve().parents[2]/"examples/basic.json").read_text())
    provider=TeXMeasurer()
    metrics=provider.measure_requests([{"id":"native-label","content":{"kind":"plain","text":"Revenue & costs"},"font_size":10,"max_width":None}])
    assert metrics["native-label"]["width"] > 0
    assert metrics["native-label"]["baseline"] > 0
    output,report=render(spec,"plain",backend="pgf",measurer=provider)
    assert report.status=="drawn"
    (tmp_path/"chart.pgf").write_text(output)
    (tmp_path/"document.tex").write_text(r"\documentclass{article}\usepackage{pgf}\begin{document}\input{chart.pgf}\end{document}")
    result=subprocess.run(["pdflatex","-interaction=nonstopmode","-halt-on-error","document.tex"],cwd=tmp_path,capture_output=True,text=True)
    assert result.returncode==0,result.stdout[-2000:]
    assert (tmp_path/"document.pdf").stat().st_size>1000
