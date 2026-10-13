"""Compatibility alias for :mod:`figkit.typst`."""
import sys
from figkit import typst as _module
sys.modules[__name__] = _module
