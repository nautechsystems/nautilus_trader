"""
Expose canonical flux strategy exports.
"""

import sys

from flux.strategies.registry import FluxStrategySpec
from flux.strategies.registry import MAKERV3_STRATEGY_SPEC
from flux.strategies.registry import get_strategy_spec
from flux.strategies.registry import get_strategy_specs
from flux.strategies.makerv3.strategy import MakerV3Strategy
from flux.strategies.makerv3.strategy import MakerV3StrategyConfig

if __name__ == "flux.strategies":
    sys.modules.setdefault("nautilus_trader.flux.strategies", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.strategies":
    sys.modules.setdefault("flux.strategies", sys.modules[__name__])


__all__ = [
    "FluxStrategySpec",
    "MAKERV3_STRATEGY_SPEC",
    "MakerV3Strategy",
    "MakerV3StrategyConfig",
    "get_strategy_spec",
    "get_strategy_specs",
]
