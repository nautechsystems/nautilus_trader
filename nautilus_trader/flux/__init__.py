"""
Compatibility shim for legacy `nautilus_trader.flux` imports.
"""

import importlib
import sys
from contextlib import suppress

from flux import *  # noqa: F403
from flux import __all__ as _flux_all
from flux import __path__ as _flux_path


__all__ = list(_flux_all)
__path__ = _flux_path

with suppress(AttributeError):
    __spec__.submodule_search_locations = __path__

for _submodule in (
    "strategies",
    "strategies.makerv3",
    "strategies.makerv3.quote_engine",
    "strategies.makerv3.rebalancing",
    "strategies.makerv3.runtime_params",
    "persistence",
    "persistence.balance_snapshots",
    "persistence.portfolio_inventory_snapshots",
    "persistence.quote_cycles",
):
    with suppress(ModuleNotFoundError):
        _module = importlib.import_module(f"flux.{_submodule}")
        sys.modules.setdefault(f"nautilus_trader.flux.{_submodule}", _module)
