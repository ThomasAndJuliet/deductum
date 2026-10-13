"""Chart stylesheets: figkit's styles (Rust ``chartgen-style``), with chartgen's style folder.

A bare style name is looked up in the bundled ``styles/`` folder (the original's charts/style/);
a ``.css`` path is read as given. The flattened text's first line names chartgen, as the
original's did. ``to_theme`` is the engine theme the Rust crate derives from the tokens.
"""
from pathlib import Path

from ._support.style import (Style as _Style, _num, contrast, from_oklch, hex_rgb,  # noqa: F401
                             label_style, luminance, mix, oklch, oklch_light, oklch_shift, parse,
                             rgb_hex, strip_comments, text_tone)

STYLE_DIR = Path(__file__).resolve().parent / "styles"


class Style(_Style):
    """A chart stylesheet, flattened; its tokens as literal values. A bare name is a style in
    the bundled styles folder."""

    maker = "chartgen"

    def __init__(self, name_or_path="plain", style_dir=None):
        super().__init__(name_or_path, STYLE_DIR if style_dir is None else style_dir)

    def to_theme(self):
        return self.theme()
