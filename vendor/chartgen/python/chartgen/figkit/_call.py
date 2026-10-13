"""Compatibility alias for :mod:`figkit._call`."""
import sys
from figkit import _call as _module
sys.modules[__name__] = _module
