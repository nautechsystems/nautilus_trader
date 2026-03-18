"""
Expose canonical flux strategy exports.
"""

import sys

from flux.strategies.equities_maker.strategy import EquitiesMakerStrategy
from flux.strategies.equities_maker.strategy import EquitiesMakerStrategyConfig
from flux.strategies.equities_taker.strategy import EquitiesTakerStrategy
from flux.strategies.equities_taker.strategy import EquitiesTakerStrategyConfig
from flux.strategies.makerv4.strategy import MakerV4Strategy
from flux.strategies.makerv4.strategy import MakerV4StrategyConfig
from flux.strategies.registry import EQUITIES_MAKER_STRATEGY_SPEC
from flux.strategies.registry import EQUITIES_TAKER_STRATEGY_SPEC
from flux.strategies.registry import FluxStrategyIdentity
from flux.strategies.registry import FluxStrategySpec
from flux.strategies.registry import MAKERV3_STRATEGY_SPEC
from flux.strategies.registry import MAKERV4_STRATEGY_SPEC
from flux.strategies.registry import get_strategy_identity
from flux.strategies.registry import get_strategy_spec
from flux.strategies.registry import get_strategy_specs
from flux.strategies.registry import resolve_strategy_spec_for_strategy_id
from flux.strategies.makerv3.strategy import MakerV3Strategy
from flux.strategies.makerv3.strategy import MakerV3StrategyConfig

_CURRENT_MODULE = sys.modules[__name__]

if __name__ == "flux.strategies":
    sys.modules["nautilus_trader.flux.strategies"] = _CURRENT_MODULE
elif __name__ == "nautilus_trader.flux.strategies":
    sys.modules["flux.strategies"] = _CURRENT_MODULE

flux_pkg = sys.modules.get("flux")
if flux_pkg is not None:
    setattr(flux_pkg, "strategies", _CURRENT_MODULE)

compat_flux_pkg = sys.modules.get("nautilus_trader.flux")
if compat_flux_pkg is not None:
    setattr(compat_flux_pkg, "strategies", _CURRENT_MODULE)


__all__ = [
    "EQUITIES_MAKER_STRATEGY_SPEC",
    "EQUITIES_TAKER_STRATEGY_SPEC",
    "EquitiesMakerStrategy",
    "EquitiesMakerStrategyConfig",
    "EquitiesTakerStrategy",
    "EquitiesTakerStrategyConfig",
    "FluxStrategyIdentity",
    "FluxStrategySpec",
    "MAKERV3_STRATEGY_SPEC",
    "MAKERV4_STRATEGY_SPEC",
    "MakerV3Strategy",
    "MakerV3StrategyConfig",
    "MakerV4Strategy",
    "MakerV4StrategyConfig",
    "get_strategy_identity",
    "get_strategy_spec",
    "get_strategy_specs",
    "resolve_strategy_spec_for_strategy_id",
]
