# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Setups Module
# -------------------------------------------------------------------------------------------------
"""Trading setup modules for VWAP Wave system."""

from vwap_wave.setups.base_setup import BaseSetup
from vwap_wave.setups.base_setup import SetupSignal
from vwap_wave.setups.base_setup import TradeDirection
from vwap_wave.setups.discovery_continuation import DiscoveryContinuationSetup
from vwap_wave.setups.fade_extremes import FadeExtremesSetup
from vwap_wave.setups.return_to_value import ReturnToValueSetup
from vwap_wave.setups.vwap_bounce import VWAPBounceSetup


__all__ = [
    "BaseSetup",
    "SetupSignal",
    "TradeDirection",
    "DiscoveryContinuationSetup",
    "FadeExtremesSetup",
    "ReturnToValueSetup",
    "VWAPBounceSetup",
]
