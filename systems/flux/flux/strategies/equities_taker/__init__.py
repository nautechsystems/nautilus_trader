"""
Expose canonical equities-taker strategy exports.
"""

from flux.strategies.equities_taker.strategy import EquitiesTakerStrategy
from flux.strategies.equities_taker.strategy import EquitiesTakerStrategyConfig


__all__ = [
    "EquitiesTakerStrategy",
    "EquitiesTakerStrategyConfig",
]
