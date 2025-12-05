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
VWAP with Standard Deviation Bands indicator.

This indicator calculates:
- VWAP (Volume Weighted Average Price)
- Upper bands at 1, 2, 3 standard deviations
- Lower bands at 1, 2, 3 standard deviations
- Resets at specified UTC hour (default 00:00 UTC for crypto)
"""

import math
from datetime import datetime

import pandas as pd

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.indicators import Indicator
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import TradeTick


class VWAPBands(Indicator):
    """
    VWAP with Standard Deviation Bands.

    Parameters
    ----------
    reset_hour_utc : int, default 0
        Hour (UTC) at which to reset VWAP. Default 0 (midnight UTC) for crypto.
    num_std_bands : int, default 3
        Number of standard deviation bands to calculate (1, 2, 3).
    """

    def __init__(
        self,
        reset_hour_utc: int = 0,
        num_std_bands: int = 3,
    ):
        PyCondition.in_range(reset_hour_utc, 0, 23, "reset_hour_utc")
        PyCondition.positive_int(num_std_bands, "num_std_bands")
        super().__init__(params=[reset_hour_utc, num_std_bands])

        self.reset_hour_utc = reset_hour_utc
        self.num_std_bands = num_std_bands

        # Internal state
        self._sum_price_volume: float = 0.0
        self._sum_volume: float = 0.0
        self._sum_price_sq_volume: float = 0.0  # For variance calculation
        self._last_reset_day: int = -1

        # Output values
        self.vwap: float = 0.0
        self.std_dev: float = 0.0

        # Bands: upper_1, upper_2, upper_3, lower_1, lower_2, lower_3
        self.upper_bands: list[float] = [0.0] * num_std_bands
        self.lower_bands: list[float] = [0.0] * num_std_bands

    def _check_reset(self, timestamp: datetime) -> None:
        """Check if VWAP should be reset based on time."""
        current_day = timestamp.timetuple().tm_yday
        current_hour = timestamp.hour

        # Reset at specified hour on new day
        if current_day != self._last_reset_day and current_hour >= self.reset_hour_utc:
            self._reset_vwap()
            self._last_reset_day = current_day

    def _reset_vwap(self) -> None:
        """Reset VWAP calculation."""
        self._sum_price_volume = 0.0
        self._sum_volume = 0.0
        self._sum_price_sq_volume = 0.0
        self.vwap = 0.0
        self.std_dev = 0.0
        self.upper_bands = [0.0] * self.num_std_bands
        self.lower_bands = [0.0] * self.num_std_bands

    def _update_vwap(self, price: float, volume: float) -> None:
        """Update VWAP and standard deviation with new data."""
        if volume <= 0:
            return

        self._sum_price_volume += price * volume
        self._sum_volume += volume
        self._sum_price_sq_volume += (price ** 2) * volume

        if self._sum_volume > 0:
            self.vwap = self._sum_price_volume / self._sum_volume

            # Calculate variance: E[X^2] - E[X]^2
            mean_sq = self._sum_price_sq_volume / self._sum_volume
            variance = mean_sq - (self.vwap ** 2)

            # Avoid negative variance due to floating point errors
            self.std_dev = math.sqrt(max(0, variance))

            # Calculate bands
            for i in range(self.num_std_bands):
                band_multiplier = i + 1
                self.upper_bands[i] = self.vwap + (band_multiplier * self.std_dev)
                self.lower_bands[i] = self.vwap - (band_multiplier * self.std_dev)

    def handle_trade_tick(self, tick: TradeTick) -> None:
        """Update the indicator with a trade tick."""
        PyCondition.not_none(tick, "tick")

        timestamp = pd.Timestamp(tick.ts_event, tz="UTC").to_pydatetime()
        self._check_reset(timestamp)

        price = tick.price.as_double()
        volume = tick.size.as_double()

        self._update_vwap(price, volume)

        if not self.initialized:
            self._set_has_inputs(True)
            self._set_initialized(True)

    def handle_bar(self, bar: Bar) -> None:
        """Update the indicator with a bar."""
        PyCondition.not_none(bar, "bar")

        timestamp = pd.Timestamp(bar.ts_event, tz="UTC").to_pydatetime()
        self._check_reset(timestamp)

        # Use typical price for VWAP
        typical_price = (bar.high.as_double() + bar.low.as_double() + bar.close.as_double()) / 3.0
        volume = bar.volume.as_double()

        self._update_vwap(typical_price, volume)

        if not self.initialized:
            self._set_has_inputs(True)
            self._set_initialized(True)

    def _reset(self) -> None:
        """Reset the indicator."""
        self._reset_vwap()
        self._last_reset_day = -1

