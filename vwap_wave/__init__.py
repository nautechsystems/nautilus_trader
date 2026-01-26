# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System
#  Copyright (C) 2024
#
#  Licensed under the Apache License, Version 2.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at http://www.apache.org/licenses/LICENSE-2.0
# -------------------------------------------------------------------------------------------------
"""
VWAP Wave Trading System.

An algorithmic trading system implementing the VWAP Wave methodology using NautilusTrader.
The system trades four distinct setups derived from Auction Market Theory:
- Price Discovery Continuation
- Fade Value Area Extremes
- Return to Value
- VWAP Bounce

The algorithm operates as a state machine that classifies market regimes (Balance vs Imbalance)
and gates setup eligibility accordingly.
"""

from vwap_wave.config.settings import VWAPWaveConfig
from vwap_wave.strategy import VWAPWaveStrategy


__all__ = [
    "VWAPWaveConfig",
    "VWAPWaveStrategy",
]
