"""Provide a thin example wrapper around canonical MakerV3 strategy exports."""

from __future__ import annotations

from nautilus_trader.flux.strategies.makerv3 import MakerV3Strategy
from nautilus_trader.flux.strategies.makerv3 import MakerV3StrategyConfig

__all__ = [
    "MakerV3Strategy",
    "MakerV3StrategyConfig",
]
