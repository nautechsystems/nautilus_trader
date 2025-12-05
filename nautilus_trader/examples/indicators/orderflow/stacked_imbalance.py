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
Stacked Imbalance Detector for order flow analysis.

Detects consecutive price levels with significant imbalance in the same direction.
Stacked imbalances indicate strong institutional activity:
- Stacked Ask Imbalances: Strong buying pressure (bullish)
- Stacked Bid Imbalances: Strong selling pressure (bearish)
"""

from collections import defaultdict
from dataclasses import dataclass
from enum import Enum
from typing import Optional

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.indicators import Indicator
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide


class ImbalanceType(Enum):
    """Type of imbalance detected."""
    NONE = 0
    ASK_IMBALANCE = 1  # More buying (bullish)
    BID_IMBALANCE = 2  # More selling (bearish)


@dataclass
class StackedImbalance:
    """Represents a detected stacked imbalance."""
    imbalance_type: ImbalanceType
    start_price: float
    end_price: float
    num_levels: int
    total_delta: float


class StackedImbalanceDetector(Indicator):
    """
    Detects stacked imbalances across consecutive price levels.

    Parameters
    ----------
    tick_size : float
        The price tick size for grouping volume.
    imbalance_ratio : float, default 3.0
        Minimum ratio to classify a level as imbalanced (e.g., 3.0 = 3:1).
    min_stack_count : int, default 3
        Minimum consecutive imbalanced levels to trigger a stacked imbalance signal.
    min_volume_per_level : float, default 0.0
        Minimum volume at a level to consider it for imbalance detection.
    """

    def __init__(
        self,
        tick_size: float,
        imbalance_ratio: float = 3.0,
        min_stack_count: int = 3,
        min_volume_per_level: float = 0.0,
    ):
        PyCondition.positive(tick_size, "tick_size")
        PyCondition.positive(imbalance_ratio, "imbalance_ratio")
        PyCondition.positive_int(min_stack_count, "min_stack_count")
        super().__init__(params=[tick_size, imbalance_ratio, min_stack_count, min_volume_per_level])

        self.tick_size = tick_size
        self.imbalance_ratio = imbalance_ratio
        self.min_stack_count = min_stack_count
        self.min_volume_per_level = min_volume_per_level

        # Volume tracking at each price level
        self._bid_volume: dict[float, float] = defaultdict(float)
        self._ask_volume: dict[float, float] = defaultdict(float)

        # Current detected stacked imbalances
        self.stacked_ask_imbalances: list[StackedImbalance] = []
        self.stacked_bid_imbalances: list[StackedImbalance] = []

        # Latest signal
        self.last_signal: ImbalanceType = ImbalanceType.NONE
        self.last_stacked_imbalance: Optional[StackedImbalance] = None

    def _round_to_tick(self, price: float) -> float:
        """Round price to nearest tick size."""
        return round(price / self.tick_size) * self.tick_size

    def handle_trade_tick(self, tick: TradeTick) -> None:
        """
        Update the detector with a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The trade tick to process.
        """
        PyCondition.not_none(tick, "tick")

        price = self._round_to_tick(tick.price.as_double())
        volume = tick.size.as_double()

        if tick.aggressor_side == AggressorSide.BUYER:
            self._ask_volume[price] += volume
        elif tick.aggressor_side == AggressorSide.SELLER:
            self._bid_volume[price] += volume

        # Detect stacked imbalances after each update
        self._detect_stacked_imbalances()

        if not self.initialized:
            self._set_has_inputs(True)
            self._set_initialized(True)

    def _get_imbalance_type(self, price: float) -> ImbalanceType:
        """Determine imbalance type at a price level."""
        ask_vol = self._ask_volume.get(price, 0.0)
        bid_vol = self._bid_volume.get(price, 0.0)
        total_vol = ask_vol + bid_vol

        if total_vol < self.min_volume_per_level:
            return ImbalanceType.NONE

        if ask_vol > 0 and bid_vol == 0:
            return ImbalanceType.ASK_IMBALANCE
        if bid_vol > 0 and ask_vol == 0:
            return ImbalanceType.BID_IMBALANCE

        if bid_vol > 0:
            ratio = ask_vol / bid_vol
            if ratio >= self.imbalance_ratio:
                return ImbalanceType.ASK_IMBALANCE
        if ask_vol > 0:
            ratio = bid_vol / ask_vol
            if ratio >= self.imbalance_ratio:
                return ImbalanceType.BID_IMBALANCE

        return ImbalanceType.NONE

    def _detect_stacked_imbalances(self) -> None:
        """Detect stacked imbalances across consecutive price levels."""
        self.stacked_ask_imbalances.clear()
        self.stacked_bid_imbalances.clear()
        self.last_signal = ImbalanceType.NONE
        self.last_stacked_imbalance = None

        # Get all price levels and sort them
        all_prices = sorted(set(self._bid_volume.keys()) | set(self._ask_volume.keys()))

        if len(all_prices) < self.min_stack_count:
            return

        # Scan for consecutive imbalances
        current_type = ImbalanceType.NONE
        stack_start = 0.0
        stack_count = 0
        stack_delta = 0.0

        for i, price in enumerate(all_prices):
            imb_type = self._get_imbalance_type(price)
            ask_vol = self._ask_volume.get(price, 0.0)
            bid_vol = self._bid_volume.get(price, 0.0)
            level_delta = ask_vol - bid_vol

            # Check if consecutive (within one tick)
            is_consecutive = (i == 0) or (abs(price - all_prices[i - 1] - self.tick_size) < self.tick_size * 0.1)

            if imb_type != ImbalanceType.NONE and imb_type == current_type and is_consecutive:
                stack_count += 1
                stack_delta += level_delta
            else:
                # Check if previous stack qualifies
                if stack_count >= self.min_stack_count:
                    stacked = StackedImbalance(
                        imbalance_type=current_type,
                        start_price=stack_start,
                        end_price=all_prices[i - 1] if i > 0 else stack_start,
                        num_levels=stack_count,
                        total_delta=stack_delta,
                    )
                    if current_type == ImbalanceType.ASK_IMBALANCE:
                        self.stacked_ask_imbalances.append(stacked)
                    else:
                        self.stacked_bid_imbalances.append(stacked)

                    self.last_signal = current_type
                    self.last_stacked_imbalance = stacked

                # Start new stack
                if imb_type != ImbalanceType.NONE:
                    current_type = imb_type
                    stack_start = price
                    stack_count = 1
                    stack_delta = level_delta
                else:
                    current_type = ImbalanceType.NONE
                    stack_count = 0
                    stack_delta = 0.0

        # Check final stack
        if stack_count >= self.min_stack_count:
            stacked = StackedImbalance(
                imbalance_type=current_type,
                start_price=stack_start,
                end_price=all_prices[-1],
                num_levels=stack_count,
                total_delta=stack_delta,
            )
            if current_type == ImbalanceType.ASK_IMBALANCE:
                self.stacked_ask_imbalances.append(stacked)
            else:
                self.stacked_bid_imbalances.append(stacked)

            self.last_signal = current_type
            self.last_stacked_imbalance = stacked

    @property
    def has_bullish_signal(self) -> bool:
        """Return True if stacked ask imbalance detected (bullish)."""
        return len(self.stacked_ask_imbalances) > 0

    @property
    def has_bearish_signal(self) -> bool:
        """Return True if stacked bid imbalance detected (bearish)."""
        return len(self.stacked_bid_imbalances) > 0

    def clear(self) -> None:
        """Clear all volume data and signals."""
        self._bid_volume.clear()
        self._ask_volume.clear()
        self.stacked_ask_imbalances.clear()
        self.stacked_bid_imbalances.clear()
        self.last_signal = ImbalanceType.NONE
        self.last_stacked_imbalance = None

    def _reset(self) -> None:
        """Reset the indicator."""
        self.clear()

