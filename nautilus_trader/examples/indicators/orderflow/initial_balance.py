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
Initial Balance (IB) indicator.

Calculates the Initial Balance range from the first hour of the NY trading session:
- IB High: Highest price during the first hour
- IB Low: Lowest price during the first hour
- IB Mid: Midpoint of IB range
- Extensions: Multiple extensions above IB High and below IB Low

Handles US daylight saving time:
- EST (Winter): NY session starts at 14:30 UTC
- EDT (Summer): NY session starts at 13:30 UTC
"""

from datetime import datetime, date
from typing import Optional

import pandas as pd

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.indicators import Indicator
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import TradeTick


def is_us_dst(dt: datetime) -> bool:
    """
    Check if a date falls within US Daylight Saving Time.

    US DST: Second Sunday of March to First Sunday of November.
    """
    year = dt.year
    # Second Sunday of March
    march_second_sunday = 14 - (date(year, 3, 1).weekday() + 1) % 7
    dst_start = datetime(year, 3, march_second_sunday, 2, 0)

    # First Sunday of November
    november_first_sunday = 7 - (date(year, 11, 1).weekday() + 1) % 7
    dst_end = datetime(year, 11, november_first_sunday, 2, 0)

    return dst_start <= dt.replace(tzinfo=None) < dst_end


class InitialBalance(Indicator):
    """
    Initial Balance indicator for the first hour of NY trading session.

    Parameters
    ----------
    num_extensions : int, default 3
        Number of extension levels above/below IB range.
    extension_multiplier : float, default 0.5
        Multiplier for each extension level (0.5 = 50% of IB range per extension).
    ib_duration_minutes : int, default 60
        Duration of Initial Balance period in minutes.
    """

    def __init__(
        self,
        num_extensions: int = 3,
        extension_multiplier: float = 0.5,
        ib_duration_minutes: int = 60,
    ):
        PyCondition.positive_int(num_extensions, "num_extensions")
        PyCondition.positive(extension_multiplier, "extension_multiplier")
        PyCondition.positive_int(ib_duration_minutes, "ib_duration_minutes")
        super().__init__(params=[num_extensions, extension_multiplier, ib_duration_minutes])

        self.num_extensions = num_extensions
        self.extension_multiplier = extension_multiplier
        self.ib_duration_minutes = ib_duration_minutes

        # Internal state
        self._current_date: Optional[date] = None
        self._ib_forming: bool = False
        self._ib_complete: bool = False
        self._ib_start_time: Optional[datetime] = None

        # Output values
        self.ib_high: float = 0.0
        self.ib_low: float = float('inf')
        self.ib_mid: float = 0.0
        self.ib_range: float = 0.0

        # Extensions above and below
        self.extensions_above: list[float] = [0.0] * num_extensions
        self.extensions_below: list[float] = [0.0] * num_extensions

    def _get_ib_start_hour_utc(self, dt: datetime) -> tuple[int, int]:
        """Get IB start time in UTC based on DST."""
        if is_us_dst(dt):
            # EDT: NY opens 9:30 AM EDT = 13:30 UTC
            return 13, 30
        else:
            # EST: NY opens 9:30 AM EST = 14:30 UTC
            return 14, 30

    def _check_ib_window(self, timestamp: datetime) -> bool:
        """Check if timestamp falls within IB window."""
        current_date = timestamp.date()

        # New day - reset IB
        if current_date != self._current_date:
            self._reset_ib()
            self._current_date = current_date

        if self._ib_complete:
            return False

        start_hour, start_min = self._get_ib_start_hour_utc(timestamp)

        # Check if we're in the IB window
        ib_start = timestamp.replace(hour=start_hour, minute=start_min, second=0, microsecond=0)
        ib_end_minutes = start_min + self.ib_duration_minutes
        ib_end_hour = start_hour + (ib_end_minutes // 60)
        ib_end_min = ib_end_minutes % 60
        ib_end = timestamp.replace(hour=ib_end_hour, minute=ib_end_min, second=0, microsecond=0)

        if ib_start <= timestamp < ib_end:
            if not self._ib_forming:
                self._ib_forming = True
                self._ib_start_time = ib_start
            return True
        elif timestamp >= ib_end and self._ib_forming:
            self._ib_complete = True
            self._ib_forming = False
            return False

        return False

    def _reset_ib(self) -> None:
        """Reset IB for new session."""
        self._ib_forming = False
        self._ib_complete = False
        self._ib_start_time = None
        self.ib_high = 0.0
        self.ib_low = float('inf')
        self.ib_mid = 0.0
        self.ib_range = 0.0
        self.extensions_above = [0.0] * self.num_extensions
        self.extensions_below = [0.0] * self.num_extensions

    def _update_ib(self, high: float, low: float) -> None:
        """Update IB range with new high/low."""
        if high > self.ib_high:
            self.ib_high = high
        if low < self.ib_low:
            self.ib_low = low

        self._calculate_extensions()

    def _calculate_extensions(self) -> None:
        """Calculate IB extensions."""
        if self.ib_high == 0 or self.ib_low == float('inf'):
            return

        self.ib_range = self.ib_high - self.ib_low
        self.ib_mid = (self.ib_high + self.ib_low) / 2.0

        extension_size = self.ib_range * self.extension_multiplier

        for i in range(self.num_extensions):
            multiplier = i + 1
            self.extensions_above[i] = self.ib_high + (extension_size * multiplier)
            self.extensions_below[i] = self.ib_low - (extension_size * multiplier)

    def handle_trade_tick(self, tick: TradeTick) -> None:
        """Update the indicator with a trade tick."""
        PyCondition.not_none(tick, "tick")

        timestamp = pd.Timestamp(tick.ts_event, tz="UTC").to_pydatetime()

        if self._check_ib_window(timestamp):
            price = tick.price.as_double()
            self._update_ib(price, price)

        if not self.initialized and self._ib_complete:
            self._set_has_inputs(True)
            self._set_initialized(True)

    def handle_bar(self, bar: Bar) -> None:
        """Update the indicator with a bar."""
        PyCondition.not_none(bar, "bar")

        timestamp = pd.Timestamp(bar.ts_event, tz="UTC").to_pydatetime()

        if self._check_ib_window(timestamp):
            self._update_ib(bar.high.as_double(), bar.low.as_double())

        if not self.initialized and self._ib_complete:
            self._set_has_inputs(True)
            self._set_initialized(True)

    @property
    def is_complete(self) -> bool:
        """Return True if IB period is complete."""
        return self._ib_complete

    @property
    def is_forming(self) -> bool:
        """Return True if IB period is currently forming."""
        return self._ib_forming

    def _reset(self) -> None:
        """Reset the indicator."""
        self._reset_ib()
        self._current_date = None

