"""
Compatibility shim for legacy `nautilus_trader.flux` imports.
"""

from contextlib import suppress

from flux import *  # noqa: F403
from flux import __all__ as _flux_all
from flux import __path__ as _flux_path


__all__ = list(_flux_all)
__path__ = _flux_path

with suppress(AttributeError):
    __spec__.submodule_search_locations = __path__
