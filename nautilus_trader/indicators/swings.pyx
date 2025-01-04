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

from collections import deque

import pandas as pd
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class Swings(Indicator):
    """
    A swing indicator which calculates and stores various swing metrics.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    """

    def __init__(self, int period):
        Condition.positive_int(period, "period")
        super().__init__(params=[period])

        self.period = period
        self._high_inputs = deque(maxlen=self.period)
        self._low_inputs = deque(maxlen=self.period)

        self.direction = 0
        self.changed = False
        self.high_datetime = None
        self.low_datetime = None
        self.high_price = 0
        self.low_price = 0
        self.length = 0
        self.duration = 0
        self.since_high = 0
        self.since_low = 0

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(
            bar.high.as_double(),
            bar.low.as_double(),
            pd.Timestamp(bar.ts_init, tz="UTC"),
        )

    cpdef void update_raw(
        self,
        double high,
        double low,
        datetime timestamp,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.
        timestamp : datetime
            The current timestamp.

        """
        # Update inputs
        self._high_inputs.append(high)
        self._low_inputs.append(low)

        # Update max high and min low
        cdef double max_high = max(self._high_inputs)
        cdef double min_low = min(self._low_inputs)

        # Calculate if swings
        cdef bint is_swing_high = high >= max_high and low >= min_low
        cdef bint is_swing_low = low <= min_low and high <= max_high

        # Swing logic
        self.changed = False

        if is_swing_high and not is_swing_low:
            if self.direction == -1:
                self.changed = True
            self.high_price = high
            self.high_datetime = timestamp
            self.direction = 1
            self.since_high = 0
            self.since_low += 1
        elif is_swing_low and not is_swing_high:
            if self.direction == 1:
                self.changed = True
            self.low_price = low
            self.low_datetime = timestamp
            self.direction = -1
            self.since_low = 0
            self.since_high += 1
        else:
            self.since_high += 1
            self.since_low += 1

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self.high_price != 0. and self.low_price != 0.0:
                self._set_initialized(True)
        # Calculate current values
        else:
            self.length = self.high_price - self.low_price
            if self.direction == 1:
                self.duration = self.since_low
            else:
                self.duration = self.since_high

    cpdef void _reset(self):
        self._high_inputs.clear()
        self._low_inputs.clear()

        self.direction = 0
        self.changed = False
        self.high_datetime = None
        self.low_datetime = None
        self.high_price = 0
        self.low_price = 0
        self.length = 0
        self.duration = 0
        self.since_high = 0
        self.since_low = 0
