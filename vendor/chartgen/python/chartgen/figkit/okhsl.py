"""Compatibility alias for :mod:`figkit.okhsl`."""
import sys
from figkit import okhsl as _module
sys.modules[__name__] = _module
