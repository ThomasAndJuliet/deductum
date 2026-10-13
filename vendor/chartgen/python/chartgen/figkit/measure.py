"""Compatibility alias for :mod:`figkit.measure`."""
import sys
from figkit import measure as _module
sys.modules[__name__] = _module
