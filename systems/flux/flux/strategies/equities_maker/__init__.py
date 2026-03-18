"""
Expose canonical equities-maker strategy exports.
"""

from flux.strategies.equities_maker.strategy import EquitiesMakerStrategy
from flux.strategies.equities_maker.strategy import EquitiesMakerStrategyConfig


__all__ = [
    "EquitiesMakerStrategy",
    "EquitiesMakerStrategyConfig",
]
