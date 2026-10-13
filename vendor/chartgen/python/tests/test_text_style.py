import shutil

import pytest

from chartgen.measure import Measurer
from chartgen.latex import TeXMeasurer
from conftest import require


def requests():
    base = {"content": {"kind": "plain", "text": "Wide words"}, "font_family": "Libertinus Serif",
            "font_size": 10, "max_width": None}
    return [
        {**base, "id": "normal", "style": {"fill": "#595959", "weight": "normal", "font_style": "normal"}},
        {**base, "id": "bold", "style": {"fill": "#595959", "weight": "bold", "font_style": "normal"}},
        {**base, "id": "italic", "style": {"fill": "#3f63b0", "weight": "normal", "font_style": "italic"}},
    ]


def test_typst_measures_selected_weight_and_slant():
    metrics = Measurer().measure_requests(requests())
    assert metrics["normal"]["width"] != metrics["bold"]["width"]
    assert metrics["normal"]["width"] != metrics["italic"]["width"]


def test_tex_measures_selected_weight_and_slant():
    require("pdflatex", "TeX Live")
    metrics = TeXMeasurer().measure_requests(requests())
    assert metrics["normal"]["width"] != metrics["bold"]["width"]
    assert metrics["normal"]["width"] != metrics["italic"]["width"]
