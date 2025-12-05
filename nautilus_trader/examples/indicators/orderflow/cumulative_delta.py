# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Cumulative Delta indicator.

Tracks the running sum of:
- Positive delta when buyers aggress (lift the ask)
- Negative delta when sellers aggress (hit the bid)

Provides:
- Cumulative delta value
- Delta per bar/period
- Buy volume and sell volume tracking
"""

from datetime import datetime
from typing import Optional

import pandas as pd

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.indicators import Indicator
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide


class CumulativeDelta(Indicator):
    """
    Cumulative Delta indicator based on trade aggressor side.

    Parameters
    ----------
    reset_hour_utc : int, default -1
        Hour (UTC) at which to reset cumulative delta. -1 for no auto-reset.
    """

    def __init__(self, reset_hour_utc: int = -1):
        super().__init__(params=[reset_hour_utc])

        self.reset_hour_utc = reset_hour_utc
        self._last_reset_day: int = -1

        # Cumulative values
        self.value: float = 0.0  # Cumulative delta
        self.buy_volume: float = 0.0  # Total buy (aggressor) volume
        self.sell_volume: float = 0.0  # Total sell (aggressor) volume

        # Per-tick delta (updated on each tick)
        self.last_delta: float = 0.0
        self.last_side: Optional[AggressorSide] = None

    def _check_reset(self, timestamp: datetime) -> None:
        """Check if delta should be reset based on time."""
        if self.reset_hour_utc < 0:
            return

        current_day = timestamp.timetuple().tm_yday
        current_hour = timestamp.hour

        if current_day != self._last_reset_day and current_hour >= self.reset_hour_utc:
            self._reset_delta()
            self._last_reset_day = current_day

    def _reset_delta(self) -> None:
        """Reset cumulative delta."""
        self.value = 0.0
        self.buy_volume = 0.0
        self.sell_volume = 0.0
        self.last_delta = 0.0
        self.last_side = None

    def handle_trade_tick(self, tick: TradeTick) -> None:
        """
        Update the indicator with a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The trade tick to process. Uses aggressor_side to determine delta direction.
        """
        PyCondition.not_none(tick, "tick")

        timestamp = pd.Timestamp(tick.ts_event, tz="UTC").to_pydatetime()
        self._check_reset(timestamp)

        volume = tick.size.as_double()
        self.last_side = tick.aggressor_side

        if tick.aggressor_side == AggressorSide.BUYER:
            # Buy aggressor - positive delta (lifting the ask)
            self.last_delta = volume
            self.value += volume
            self.buy_volume += volume
        elif tick.aggressor_side == AggressorSide.SELLER:
            # Sell aggressor - negative delta (hitting the bid)
            self.last_delta = -volume
            self.value -= volume
            self.sell_volume += volume
        else:
            # No aggressor - neutral
            self.last_delta = 0.0

        if not self.initialized:
            self._set_has_inputs(True)
            self._set_initialized(True)

    @property
    def delta_ratio(self) -> float:
        """
        Return the buy/sell volume ratio.

        Returns positive if more buying pressure, negative if more selling pressure.
        """
        total = self.buy_volume + self.sell_volume
        if total == 0:
            return 0.0
        return (self.buy_volume - self.sell_volume) / total

    @property
    def buy_sell_ratio(self) -> float:
        """Return buy volume / sell volume ratio."""
        if self.sell_volume == 0:
            return float('inf') if self.buy_volume > 0 else 0.0
        return self.buy_volume / self.sell_volume

    def _reset(self) -> None:
        """Reset the indicator."""
        self._reset_delta()
        self._last_reset_day = -1

