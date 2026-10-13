"""Compatibility alias for standalone figure styles."""
import sys
from figkit import style as _module
sys.modules[__name__] = _module
