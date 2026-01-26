# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Execution Module
# -------------------------------------------------------------------------------------------------
"""Execution and trade management modules for VWAP Wave system."""

from vwap_wave.execution.order_factory import VWAPWaveOrderFactory
from vwap_wave.execution.trade_manager import ManagedTrade
from vwap_wave.execution.trade_manager import TradeManager
from vwap_wave.execution.trade_manager import TradeState


__all__ = [
    "TradeManager",
    "TradeState",
    "ManagedTrade",
    "VWAPWaveOrderFactory",
]
