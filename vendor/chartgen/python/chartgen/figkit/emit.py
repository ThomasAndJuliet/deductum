"""Compatibility alias for :mod:`figkit.emit`."""
import sys
from figkit import emit as _module
sys.modules[__name__] = _module
