# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import cython
from cpython.datetime cimport datetime
from collections import deque
from typing import List

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class Swings(Indicator):
    """
    A swing indicator which calculates and stores various swing metrics.
    """

    def __init__(self, int period, bint check_inputs=False):
        """
        Initializes a new instance of the Swings class.

        :param period: The rolling window period for the indicator (> 0).
        :param check_inputs: The flag indicating whether the input values should be checked.
        """
        Condition.positive_int(period, 'period')

        super().__init__(params=[period], check_inputs=check_inputs)
        self.period = period
        self._high_inputs = deque(maxlen=self.period)
        self._low_inputs = deque(maxlen=self.period)

        self.value = 0
        self.direction = 0
        self.changed = False
        self.high_datetime = datetime(1970, 1, 1, 0, 0, 0)
        self.low_datetime = datetime(1970, 1, 1, 0, 0, 0)
        self.lengths = []    # type: List[double]
        self.durations = []  # type: List[double]
        self.high_price = 0.0
        self.low_price = 0.0
        self.length_last = 0.0
        self.length_current = 0.0
        self.duration_last = 0
        self.duration_current = 0
        self.since_high = 0
        self.since_low = 0

    @cython.binding(True)
    cpdef void update(
            self,
            double high,
            double low,
            datetime timestamp):
        """
        Update the indicator with the given values.

        :param high: The high price (> 0).
        :param low: The low price (> 0).
        :param timestamp: The timestamp.
        """
        if self.check_inputs:
            Condition.positive(high, 'high')
            Condition.positive(low, 'low')
            Condition.true(high >= low, 'high >= low')

        self._calculate_swing_logic(high, low, timestamp)

    cdef void _calculate_swing_logic(
            self,
            double high,
            double low,
            datetime timestamp):
        """
        Calculate the swing logic based on the given prices.

        :param high: The high price of the last closed bar. 
        :param low: The low price of the last closed bar.
        :param timestamp: The timestamp of the last closed bar.
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
                self._swing_changed()
            self.high_price = high
            self.high_datetime = timestamp
            self.direction = 1
            self.value = 1
            self.since_high = 0
            self.since_low += 1
        elif is_swing_low and not is_swing_high:
            if self.direction == 1:
                self._swing_changed()
            self.low_price = low
            self.low_datetime = timestamp
            self.direction = -1
            self.value = -1
            self.since_low = 0
            self.since_high += 1
        else:
            self.value = 0
            self.since_high += 1
            self.since_low += 1

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs()
            if self.high_price != 0. and self.low_price != 0.0:
                self._set_initialized()
        # Calculate current values
        else:
            self.length_current = self.high_price - self.low_price
            if self.direction == 1:
                self.duration_current = self.since_low
            else:
                self.duration_current = self.since_high

    cdef void _swing_changed(self):
        self.length_last = self.length_current
        self.lengths.append(self.length_current)
        self.duration_last = self.duration_current
        self.durations.append(self.duration_current)
        self.changed = True

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._high_inputs.clear()
        self._low_inputs.clear()

        self.value = 0
        self.direction = 0
        self.changed = False
        self.high_datetime = datetime(1970, 1, 1, 0, 0, 0)
        self.low_datetime = datetime(1970, 1, 1, 0, 0, 0)
        self.lengths.clear()
        self.durations.clear()
        self.high_price = 0.0
        self.low_price = 0.0
        self.length_last = 0.0
        self.length_current = 0.0
        self.duration_last = 0
        self.duration_current = 0
        self.since_high = 0
        self.since_low = 0
