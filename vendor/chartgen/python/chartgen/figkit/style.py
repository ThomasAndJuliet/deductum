"""Compatibility alias for :mod:`figkit.style`."""
import sys
from figkit import style as _module
sys.modules[__name__] = _module
