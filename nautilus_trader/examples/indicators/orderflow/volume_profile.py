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
Volume Profile indicator with POC, VAL, VAH, and HVN/LVN detection.

This indicator aggregates volume at each price level and calculates:
- POC (Point of Control): Price level with the highest volume
- VAH (Value Area High): Upper bound of the value area (default 70% of volume)
- VAL (Value Area Low): Lower bound of the value area
- HVN (High Volume Nodes): Price levels with significantly high volume
- LVN (Low Volume Nodes): Price levels with significantly low volume
"""

from collections import defaultdict
from datetime import datetime
from typing import Optional

import pandas as pd

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.indicators import Indicator
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import TradeTick


class VolumeProfile(Indicator):
    """
    Volume Profile indicator that tracks volume distribution across price levels.

    Parameters
    ----------
    tick_size : float
        The price tick size for grouping volume (e.g., 0.01, 1.0, 10.0).
    value_area_pct : float, default 0.70
        The percentage of total volume to include in the value area.
    hvn_threshold : float, default 1.5
        Multiplier above average volume to classify as HVN.
    lvn_threshold : float, default 0.5
        Multiplier below average volume to classify as LVN.
    reset_hour_utc : int, default 0
        Hour (UTC) at which to reset the profile (0-23). Set to -1 for no auto-reset.
    """

    def __init__(
        self,
        tick_size: float,
        value_area_pct: float = 0.70,
        hvn_threshold: float = 1.5,
        lvn_threshold: float = 0.5,
        reset_hour_utc: int = 0,
    ):
        PyCondition.positive(tick_size, "tick_size")
        PyCondition.in_range(value_area_pct, 0.0, 1.0, "value_area_pct")
        super().__init__(params=[tick_size, value_area_pct, hvn_threshold, lvn_threshold, reset_hour_utc])

        self.tick_size = tick_size
        self.value_area_pct = value_area_pct
        self.hvn_threshold = hvn_threshold
        self.lvn_threshold = lvn_threshold
        self.reset_hour_utc = reset_hour_utc

        # Volume at each price level
        self._volume_at_price: dict[float, float] = defaultdict(float)
        self._last_reset_day: int = -1

        # Incremental POC tracking (O(1) updates)
        self._poc_price: float = 0.0
        self._poc_volume: float = 0.0

        # Cached computed values (lazy evaluation)
        self._cached_vah: float = 0.0
        self._cached_val: float = 0.0
        self._cached_hvn_levels: list[float] = []
        self._cached_lvn_levels: list[float] = []

        # Dirty flags for lazy evaluation
        self._value_area_dirty: bool = True
        self._volume_nodes_dirty: bool = True

        self.total_volume: float = 0.0

    def _round_to_tick(self, price: float) -> float:
        """Round price to nearest tick size."""
        return round(price / self.tick_size) * self.tick_size

    def _check_reset(self, timestamp: datetime) -> None:
        """Check if profile should be reset based on time."""
        if self.reset_hour_utc < 0:
            return

        current_day = timestamp.timetuple().tm_yday
        current_hour = timestamp.hour

        # Reset at specified hour on new day
        if current_day != self._last_reset_day and current_hour >= self.reset_hour_utc:
            self._reset_profile()
            self._last_reset_day = current_day

    def _reset_profile(self) -> None:
        """Reset the volume profile data."""
        self._volume_at_price.clear()
        self.total_volume = 0.0
        self._poc_price = 0.0
        self._poc_volume = 0.0
        self._cached_vah = 0.0
        self._cached_val = 0.0
        self._cached_hvn_levels = []
        self._cached_lvn_levels = []
        self._value_area_dirty = True
        self._volume_nodes_dirty = True

    def handle_trade_tick(self, tick: TradeTick) -> None:
        """Update the indicator with a trade tick."""
        PyCondition.not_none(tick, "tick")

        timestamp = pd.Timestamp(tick.ts_event, tz="UTC").to_pydatetime()
        self._check_reset(timestamp)

        price = self._round_to_tick(tick.price.as_double())
        volume = tick.size.as_double()

        self._volume_at_price[price] += volume
        self.total_volume += volume

        # Incremental POC update (O(1) instead of O(n))
        if self._volume_at_price[price] > self._poc_volume:
            self._poc_price = price
            self._poc_volume = self._volume_at_price[price]

        # Mark cached values as dirty (lazy evaluation)
        self._value_area_dirty = True
        self._volume_nodes_dirty = True

        if not self.initialized:
            self._set_has_inputs(True)
            self._set_initialized(True)

    def handle_bar(self, bar: Bar) -> None:
        """Update the indicator with a bar (uses typical price)."""
        PyCondition.not_none(bar, "bar")

        timestamp = pd.Timestamp(bar.ts_event, tz="UTC").to_pydatetime()
        self._check_reset(timestamp)

        # Use typical price for bar-based volume profile
        typical_price = (bar.high.as_double() + bar.low.as_double() + bar.close.as_double()) / 3.0
        price = self._round_to_tick(typical_price)
        volume = bar.volume.as_double()

        self._volume_at_price[price] += volume
        self.total_volume += volume

        # Incremental POC update (O(1) instead of O(n))
        if self._volume_at_price[price] > self._poc_volume:
            self._poc_price = price
            self._poc_volume = self._volume_at_price[price]

        # Mark cached values as dirty (lazy evaluation)
        self._value_area_dirty = True
        self._volume_nodes_dirty = True

        if not self.initialized:
            self._set_has_inputs(True)
            self._set_initialized(True)

    # Properties with lazy evaluation
    @property
    def poc(self) -> float:
        """Point of Control (price with highest volume) - computed incrementally."""
        return self._poc_price

    @property
    def vah(self) -> float:
        """Value Area High - computed lazily when accessed."""
        if self._value_area_dirty:
            self._calculate_value_area()
        return self._cached_vah

    @property
    def val(self) -> float:
        """Value Area Low - computed lazily when accessed."""
        if self._value_area_dirty:
            self._calculate_value_area()
        return self._cached_val

    @property
    def hvn_levels(self) -> list[float]:
        """High Volume Nodes - computed lazily when accessed."""
        if self._volume_nodes_dirty:
            self._calculate_volume_nodes()
        return self._cached_hvn_levels

    @property
    def lvn_levels(self) -> list[float]:
        """Low Volume Nodes - computed lazily when accessed."""
        if self._volume_nodes_dirty:
            self._calculate_volume_nodes()
        return self._cached_lvn_levels

    def _calculate_value_area(self) -> None:
        """Calculate Value Area High and Low (lazy evaluation)."""
        if self.total_volume == 0:
            self._cached_vah = 0.0
            self._cached_val = 0.0
            self._value_area_dirty = False
            return

        target_volume = self.total_volume * self.value_area_pct
        sorted_prices = sorted(self._volume_at_price.keys())

        if not sorted_prices:
            self._cached_vah = 0.0
            self._cached_val = 0.0
            self._value_area_dirty = False
            return

        # Start from POC and expand outward
        try:
            poc_idx = sorted_prices.index(self._poc_price)
        except ValueError:
            # POC not in sorted prices (shouldn't happen, but handle gracefully)
            poc_idx = 0

        accumulated_volume = self._volume_at_price[sorted_prices[poc_idx]]

        low_idx = poc_idx
        high_idx = poc_idx

        while accumulated_volume < target_volume:
            # Get volume above and below current range
            vol_above = self._volume_at_price.get(sorted_prices[high_idx + 1], 0) if high_idx + 1 < len(sorted_prices) else 0
            vol_below = self._volume_at_price.get(sorted_prices[low_idx - 1], 0) if low_idx > 0 else 0

            if vol_above == 0 and vol_below == 0:
                break

            # Expand in direction of higher volume
            if vol_above >= vol_below and high_idx + 1 < len(sorted_prices):
                high_idx += 1
                accumulated_volume += vol_above
            elif low_idx > 0:
                low_idx -= 1
                accumulated_volume += vol_below
            elif high_idx + 1 < len(sorted_prices):
                high_idx += 1
                accumulated_volume += vol_above
            else:
                break

        self._cached_val = sorted_prices[low_idx]
        self._cached_vah = sorted_prices[high_idx]
        self._value_area_dirty = False

    def _calculate_volume_nodes(self) -> None:
        """Calculate High Volume Nodes and Low Volume Nodes (lazy evaluation)."""
        if not self._volume_at_price:
            self._cached_hvn_levels = []
            self._cached_lvn_levels = []
            self._volume_nodes_dirty = False
            return

        avg_volume = self.total_volume / len(self._volume_at_price)

        self._cached_hvn_levels = [
            price for price, volume in self._volume_at_price.items()
            if volume >= avg_volume * self.hvn_threshold
        ]

        self._cached_lvn_levels = [
            price for price, volume in self._volume_at_price.items()
            if volume <= avg_volume * self.lvn_threshold
        ]

        self._volume_nodes_dirty = False

    def get_volume_at_price(self, price: float) -> float:
        """Get volume at a specific price level."""
        rounded_price = self._round_to_tick(price)
        return self._volume_at_price.get(rounded_price, 0.0)

    def get_profile_dict(self) -> dict[float, float]:
        """Return a copy of the volume profile dictionary."""
        return dict(self._volume_at_price)

    def _reset(self) -> None:
        """Reset the indicator."""
        self._reset_profile()
        self._last_reset_day = -1

