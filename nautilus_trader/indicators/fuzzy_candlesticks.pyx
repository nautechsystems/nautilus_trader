# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import sys
import cython
import numpy as np
from libc.math cimport fabs
from collections import deque

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport fast_mean
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.indicators.fuzzy_enums.candle_body cimport CandleBodySize
from nautilus_trader.indicators.fuzzy_enums.candle_direction cimport CandleDirection
from nautilus_trader.indicators.fuzzy_enums.candle_size cimport CandleSize
from nautilus_trader.indicators.fuzzy_enums.candle_wick cimport CandleWickSize

cdef double _MAX_FLOAT = sys.float_info.max


cdef class FuzzyMembership:
    """
    Defines the attributes of a fuzzy membership function. Used internally by the
    indicator to fuzzify crisp values to linguistic variables.
    """

    def __init__(self,
                 int linguistic_variable,
                 double x1,
                 double x2):

        self.linguistic_variable = linguistic_variable
        self.x1 = x1
        self.x2 = x2


cdef class FuzzyCandle:
    """
    Represents a fuzzy candle.
    """
    def __init__(self,
                 CandleDirection direction,
                 CandleSize size,
                 CandleBodySize body_size,
                 CandleWickSize upper_wick_size,
                 CandleWickSize lower_wick_size):

        self.direction = direction
        self.size = size
        self.body_size = body_size
        self.upper_wick_size = upper_wick_size
        self.lower_wick_size = lower_wick_size

    def __eq__(self, FuzzyCandle other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.

        """
        return self.direction == other.direction \
            and self.size == other.size \
            and self.body_size == other.body_size \
            and self.upper_wick_size == other.upper_wick_size \
            and self.lower_wick_size == other.lower_wick_size

    def __ne__(self, FuzzyCandle other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.

        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
         Return the hash code of this object.

        :return int.

        """
        return hash(self.direction + self.size + self.body_size + self.upper_wick_size + self.lower_wick_size)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.

        """
        return f"({self.direction}, {self.size}, {self.body_size}, {self.lower_wick_size}, {self.upper_wick_size})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}{str(self)} object at {id(self)}>"


cdef class FuzzyCandlesticks(Indicator):
    """
    An indicator which fuzzifies bar data to produce fuzzy candlesticks.
    Bar data is dimensionally reduced via fuzzy feature extraction.

    Attributes
    ----------
    value : FuzzyCandle
        The last fuzzy candle to close.
    vector : list of int
        A list representing the fuzzified features of the last fuzzy candle.
        [0] direction: int (-1, 0, 1)
        [1] size: int (0 to 6)
        [2] body size: int (0 to 4)
        [3] upper wick size: int (0 to 3)
        [4] lower wick size: int (0 to 3)

    """

    def __init__(self,
                 int period=10,
                 double threshold1=0.5,
                 double threshold2=1.0,
                 double threshold3=2.0,
                 double threshold4=3.0,
                 bint check_inputs=False):
        """
        Initializes a new instance of the FuzzyCandlesticks class.

        Parameters
        ----------
        period : int
            The rolling window period for the indicator (> 0).
        threshold1 : float
            The membership function x threshold1 (>= 0).
        threshold2 : float
            The membership function x threshold2 (> threshold1).
        threshold3 : float
            The membership function x threshold3 (> threshold2).
        threshold4 : float
            The membership function x threshold4 (> threshold3).
        check_inputs : bool
            The flag indicating whether the input values should be checked.
        """
        Condition.positive_int(period, 'period')
        Condition.positive(threshold1, 'threshold1')
        Condition.true(threshold2 > threshold1, 'threshold2 > threshold1')
        Condition.true(threshold3 > threshold2, 'threshold3 > threshold2')
        Condition.true(threshold4 > threshold3, 'threshold4 > threshold3')

        super().__init__(params=[period,
                                 threshold1,
                                 threshold2,
                                 threshold3,
                                 threshold4],
                         check_inputs=check_inputs)

        self.period = period
        self._threshold1 = threshold1
        self._threshold2 = threshold2
        self._threshold3 = threshold3
        self._threshold4 = threshold4
        self._lengths = deque(maxlen=self.period)
        self._body_percents = deque(maxlen=self.period)
        self._upper_wick_percents = deque(maxlen=self.period)
        self._lower_wick_percents = deque(maxlen=self.period)
        self._last_open = 0.0
        self._last_high = 0.0
        self._last_low = 0.0
        self._last_close = 0.0

        self.vector = None
        self.value = None

    @cython.binding(True)  # Needed for IndicatorUpdater to use this method as a delegate
    cpdef void update(
            self,
            double open_price,
            double high_price,
            double low_price,
            double close_price) except *:
        """
        Update the indicator with the given values, data should be
        pre-cleaned to ensure it is not invalid.

        :param open_price: The open price (> 0).
        :param high_price: The high price (> 0).
        :param low_price: The low price (> 0).
        :param close_price: The close price (> 0).
        """
        if self.check_inputs:
            Condition.positive(open_price, 'open_price')
            Condition.positive(high_price, 'high_price')
            Condition.positive(low_price, 'low_price')
            Condition.positive(close_price, 'close_price')
            Condition.true(high_price >= low_price, 'high_price >= low_price')
            Condition.true(high_price >= close_price, 'high_price >= close_price')
            Condition.true(low_price <= close_price, 'low_price <= close_price')

        # Check if this is the first input
        if not self.has_inputs:
            self._last_open = open_price
            self._last_high = high_price
            self._last_low = low_price
            self._last_close = close_price

        # Update last prices
        self._last_open = open_price
        self._last_high = high_price
        self._last_low = low_price
        self._last_close = close_price

        # Update measurements
        self._lengths.append(fabs(high_price - low_price))

        if self._lengths[0] == 0.0:
            self._body_percents.append(0.0)
            self._upper_wick_percents.append(0.0)
            self._lower_wick_percents.append(0.0)
        else:
            self._body_percents.append(fabs(open_price - low_price / self._lengths[0]))
            self._upper_wick_percents.append((high_price - max(open_price, close_price)) / self._lengths[0])
            self._lower_wick_percents.append((min(open_price, close_price) - low_price) / self._lengths[0])

        # Calculate statistics for bars
        cdef double mean_length = fast_mean(list(self._lengths))
        cdef double mean_body_percent = fast_mean(list(self._body_percents))
        cdef double mean_upper_wick = fast_mean(list(self._upper_wick_percents))
        cdef double mean_lower_wick = fast_mean(list(self._lower_wick_percents))

        cdef double sd_lengths = float(np.std(self._lengths))
        cdef double sd_body_percents = float(np.std(self._body_percents))
        cdef double sd_upper_wick_percents = float(np.std(self._upper_wick_percents))
        cdef double sd_lower_wick_percents = float(np.std(self._lower_wick_percents))

        # Create fuzzy candle
        self.value = FuzzyCandle(
            direction=self._fuzzify_direction(open_price, close_price),
            size=self._fuzzify_size(
                self._lengths[0],
                mean_length,
                sd_lengths),
            body_size=self._fuzzify_body_size(
                self._body_percents[0],
                mean_body_percent,
                sd_body_percents),
            upper_wick_size=self._fuzzify_wick_size(
                self._upper_wick_percents[0],
                mean_upper_wick,
                sd_upper_wick_percents),
            lower_wick_size=self._fuzzify_wick_size(
                self._lower_wick_percents[0],
                mean_lower_wick,
                sd_lower_wick_percents))

        # Create fuzzy candle as np array
        self.vector = [
            self.value.direction,
            self.value.size,
            self.value.body_size,
            self.value.upper_wick_size,
            self.value.lower_wick_size
        ]

        # Initialization logic
        if self.initialized is False:
            self._set_has_inputs(True)
            if len(self._lengths) >= self.period:
                self._set_initialized(True)

    cdef CandleDirection _fuzzify_direction(self, double open_price, double close_price):
        """
        Fuzzify the candle direction from the given inputs.

        :param open_price: The open price of the bar (> 0).
        :param close_price: The close price of the bar (> 0).
        :return: The fuzzified direction of the bar.
        """
        if close_price > open_price:
            return CandleDirection.BULL
        if close_price < open_price:
            return CandleDirection.BEAR
        else:
            return CandleDirection.NONE

    cdef CandleSize _fuzzify_size(
            self,
            double length,
            double mean_length,
            double sd_lengths):
        """
        Fuzzify the candle size from the given inputs.

        :param length: The length of the bar (>= 0).
        :param mean_length: The mean length of bars (>= 0).
        :param sd_lengths: The standard deviation of bar lengths (>= 0).
        :return: The fuzzy candle size from the given inputs.
        """
        if length == 0.0:
            return CandleSize.NONE

        cdef list fuzzy_size_table = [
            FuzzyMembership(
                linguistic_variable=CandleSize.VERY_SMALL,
                x1=0.0,
                x2=mean_length - (sd_lengths * self._threshold2)),
            FuzzyMembership(
                linguistic_variable=CandleSize.SMALL,
                x1=mean_length - (sd_lengths * self._threshold2),
                x2=mean_length + (sd_lengths * self._threshold1)),
            FuzzyMembership(
                linguistic_variable=CandleSize.MEDIUM,
                x1=mean_length + (sd_lengths * self._threshold1),
                x2=sd_lengths * self._threshold2),
            FuzzyMembership(
                linguistic_variable=CandleSize.LARGE,
                x1=mean_length + (sd_lengths * self._threshold2),
                x2=mean_length + (sd_lengths * self._threshold3)),
            FuzzyMembership(
                linguistic_variable=CandleSize.VERY_LARGE,
                x1=mean_length + (sd_lengths * self._threshold3),
                x2=mean_length + (sd_lengths * self._threshold4)),
            FuzzyMembership(
                linguistic_variable=CandleSize.EXTREMELY_LARGE,
                x1=mean_length + (sd_lengths * self._threshold4),
                x2=_MAX_FLOAT)]

        for fuzzy_size in fuzzy_size_table:
            if fuzzy_size.x1 <= length <= fuzzy_size.x2:
                return fuzzy_size.linguistic_variable

    cdef CandleBodySize _fuzzify_body_size(
            self,
            double body_percent,
            double mean_body_percent,
            double sd_body_percents):
        """
        Fuzzify the candle body size from the given inputs.

        :param body_percent: The percent of the bar the body constitutes (>= 0).
        :param mean_body_percent: The mean of body percents (>= 0).
        :param sd_body_percents: The standard deviation of body percents (>= 0).
        :return: The fuzzy body size from the given inputs.
        """
        if body_percent == 0.0:
            return CandleBodySize.NONE

        cdef list fuzzy_body_size_table = [
            FuzzyMembership(
                linguistic_variable=CandleBodySize.SMALL,
                x1=0.0,
                x2=mean_body_percent - (sd_body_percents * self._threshold1)),
            FuzzyMembership(
                linguistic_variable=CandleBodySize.MEDIUM,
                x1=mean_body_percent - (sd_body_percents * self._threshold1),
                x2=mean_body_percent + (sd_body_percents * self._threshold1)),
            FuzzyMembership(
                linguistic_variable=CandleBodySize.LARGE,
                x1=mean_body_percent + (sd_body_percents * self._threshold1),
                x2=mean_body_percent + (sd_body_percents * self._threshold2)),
            FuzzyMembership(
                linguistic_variable=CandleBodySize.TREND,
                x1=mean_body_percent + (sd_body_percents * self._threshold2),
                x2=_MAX_FLOAT)]

        for fuzzy_body_size in fuzzy_body_size_table:
            if fuzzy_body_size.x1 <= body_percent <= fuzzy_body_size.x2:
                return fuzzy_body_size.linguistic_variable

    cdef CandleWickSize _fuzzify_wick_size(
            self,
            double wick_percent,
            double mean_wick_percent,
            double sd_wick_percents):
        """
        Fuzzify the candle wick size from the given inputs.

        :param wick_percent: The percent of the bar the wick constitutes (>= 0).
        :param mean_wick_percent: The mean wick percents (>= 0).
        :param sd_wick_percents: The standard deviation of wick percents (>= 0).
        :return: The fuzzy wick size from the given inputs.
        """
        if wick_percent == 0.0:
            return CandleWickSize.NONE

        cdef list fuzzy_wick_size_table = [
            FuzzyMembership(
                linguistic_variable=CandleWickSize.SMALL,
                x1=0.0,
                x2=mean_wick_percent - (sd_wick_percents * self._threshold1)),
            FuzzyMembership(
                linguistic_variable=CandleWickSize.MEDIUM,
                x1=mean_wick_percent - (sd_wick_percents * self._threshold1),
                x2=mean_wick_percent + (sd_wick_percents * self._threshold2)),
            FuzzyMembership(
                linguistic_variable=CandleWickSize.LARGE,
                x1=mean_wick_percent + (sd_wick_percents * self._threshold2),
                x2=_MAX_FLOAT)]

        for fuzzy_wick_size in fuzzy_wick_size_table:
            if fuzzy_wick_size.x1 <= wick_percent <= fuzzy_wick_size.x2:
                return fuzzy_wick_size.linguistic_variable

    cpdef void reset(self) except *:
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._lengths.clear()
        self._body_percents.clear()
        self._upper_wick_percents.clear()
        self._lower_wick_percents.clear()
        self._last_open = 0.0
        self._last_high = 0.0
        self._last_low = 0.0
        self._last_close = 0.0
        self.vector = None
        self.value = None
