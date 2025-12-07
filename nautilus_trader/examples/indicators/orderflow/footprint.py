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
Footprint Aggregator for order flow analysis.

Aggregates trade data into a footprint structure showing:
- Bid volume (sellers hitting the bid)
- Ask volume (buyers lifting the ask)
- Delta at each price level
- Imbalance detection at each level
"""

from collections import defaultdict
from dataclasses import dataclass, field
from typing import Optional

import pandas as pd

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.indicators import Indicator
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide


@dataclass
class FootprintLevel:
    """Data at a single price level in the footprint."""
    bid_volume: float = 0.0  # Volume from sell aggressors
    ask_volume: float = 0.0  # Volume from buy aggressors
    trade_count: int = 0

    @property
    def delta(self) -> float:
        """Delta at this level (ask_volume - bid_volume)."""
        return self.ask_volume - self.bid_volume

    @property
    def total_volume(self) -> float:
        """Total volume at this level."""
        return self.bid_volume + self.ask_volume

    @property
    def imbalance_ratio(self) -> float:
        """
        Imbalance ratio at this level.
        Returns: -1 to 1 where positive = more ask volume, negative = more bid volume.
        """
        total = self.total_volume
        if total == 0:
            return 0.0
        return self.delta / total


class FootprintAggregator(Indicator):
    """
    Aggregates trade ticks into footprint candle data.

    Parameters
    ----------
    tick_size : float
        The price tick size for grouping volume.
    imbalance_threshold : float, default 3.0
        Ratio threshold to classify a level as imbalanced (e.g., 3.0 = 3:1 ratio).
    """

    def __init__(
        self,
        tick_size: float,
        imbalance_threshold: float = 3.0,
    ):
        PyCondition.positive(tick_size, "tick_size")
        PyCondition.positive(imbalance_threshold, "imbalance_threshold")
        super().__init__(params=[tick_size, imbalance_threshold])

        self.tick_size = tick_size
        self.imbalance_threshold = imbalance_threshold

        # Footprint data: price -> FootprintLevel
        self._levels: dict[float, FootprintLevel] = defaultdict(FootprintLevel)

        # Current candle boundaries
        self.high: float = 0.0
        self.low: float = float('inf')
        self.open: float = 0.0
        self.close: float = 0.0

        # Aggregated values
        self.total_delta: float = 0.0
        self.total_volume: float = 0.0
        self.buy_volume: float = 0.0
        self.sell_volume: float = 0.0

        # POC for current footprint (incremental tracking - already optimized)
        self.poc_price: float = 0.0
        self.poc_volume: float = 0.0

        # Cached imbalanced levels (lazy evaluation)
        self._cached_imbalanced_levels: dict[float, str] = {}
        self._imbalanced_levels_dirty: bool = True

    def _round_to_tick(self, price: float) -> float:
        """Round price to nearest tick size."""
        return round(price / self.tick_size) * self.tick_size

    def handle_trade_tick(self, tick: TradeTick) -> None:
        """
        Update the footprint with a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The trade tick to process.
        """
        PyCondition.not_none(tick, "tick")

        price = tick.price.as_double()
        rounded_price = self._round_to_tick(price)
        volume = tick.size.as_double()

        # Update OHLC
        if self.open == 0.0:
            self.open = price
        self.close = price
        if price > self.high:
            self.high = price
        if price < self.low:
            self.low = price

        # Get or create level
        level = self._levels[rounded_price]
        level.trade_count += 1

        # Update volume based on aggressor side
        if tick.aggressor_side == AggressorSide.BUYER:
            level.ask_volume += volume
            self.buy_volume += volume
            self.total_delta += volume
        elif tick.aggressor_side == AggressorSide.SELLER:
            level.bid_volume += volume
            self.sell_volume += volume
            self.total_delta -= volume

        self.total_volume += volume

        # Update POC (incremental - O(1))
        if level.total_volume > self.poc_volume:
            self.poc_volume = level.total_volume
            self.poc_price = rounded_price

        # Mark imbalanced levels as dirty (lazy evaluation)
        self._imbalanced_levels_dirty = True

        if not self.initialized:
            self._set_has_inputs(True)
            self._set_initialized(True)

    def get_level(self, price: float) -> FootprintLevel:
        """Get footprint data at a specific price level."""
        rounded_price = self._round_to_tick(price)
        return self._levels.get(rounded_price, FootprintLevel())

    def get_imbalanced_levels(self) -> dict[float, str]:
        """
        Get all price levels with significant imbalance (lazy evaluation).

        Returns
        -------
        dict[float, str]
            Price -> 'BID' or 'ASK' indicating which side dominates.
        """
        if self._imbalanced_levels_dirty:
            self._calculate_imbalanced_levels()
        return self._cached_imbalanced_levels

    def _calculate_imbalanced_levels(self) -> None:
        """Calculate imbalanced levels (only when needed)."""
        imbalances = {}
        for price, level in self._levels.items():
            if level.ask_volume > 0 and level.bid_volume > 0:
                ratio = level.ask_volume / level.bid_volume
                if ratio >= self.imbalance_threshold:
                    imbalances[price] = 'ASK'
                elif ratio <= 1 / self.imbalance_threshold:
                    imbalances[price] = 'BID'
            elif level.ask_volume > 0 and level.bid_volume == 0:
                imbalances[price] = 'ASK'
            elif level.bid_volume > 0 and level.ask_volume == 0:
                imbalances[price] = 'BID'

        self._cached_imbalanced_levels = imbalances
        self._imbalanced_levels_dirty = False

    def get_all_levels(self) -> dict[float, FootprintLevel]:
        """Return a copy of all footprint levels."""
        return dict(self._levels)

    def clear_footprint(self) -> None:
        """Clear the current footprint data (call at end of candle period)."""
        self._levels.clear()
        self.high = 0.0
        self.low = float('inf')
        self.open = 0.0
        self.close = 0.0
        self.total_delta = 0.0
        self.total_volume = 0.0
        self.buy_volume = 0.0
        self.sell_volume = 0.0
        self.poc_price = 0.0
        self.poc_volume = 0.0
        self._cached_imbalanced_levels = {}
        self._imbalanced_levels_dirty = True

    def _reset(self) -> None:
        """Reset the indicator."""
        self.clear_footprint()

