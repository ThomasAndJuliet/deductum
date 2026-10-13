"""Compatibility alias for :mod:`figkit.model`."""
import sys
from figkit import model as _module
sys.modules[__name__] = _module
