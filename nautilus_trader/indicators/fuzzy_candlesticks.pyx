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
import sys
import numpy as np

from enum import Enum, unique
from collections import deque
from typing import NamedTuple

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.core.correctness cimport Condition


cdef double _MAX_FLOAT = sys.float_info.max


@unique
class CandleDirection(Enum):
    BULL = 1
    NONE = 0  # Doji
    BEAR = -1


@unique
class CandleSize(Enum):
    NONE = 0  # Doji
    VERY_SMALL = 1
    SMALL = 2
    MEDIUM = 3
    LARGE = 4
    VERY_LARGE = 5
    EXTREMELY_LARGE = 6


@unique
class CandleBodySize(Enum):
    NONE = 0  # Doji
    SMALL = 1
    MEDIUM = 2
    LARGE = 3
    TREND = 4


@unique
class CandleWickSize(Enum):
    NONE = 0  # No candle wick
    SMALL = 1
    MEDIUM = 2
    LARGE = 3


# Defines the attributes of a fuzzy membership function. Used internally by the
# indicator to fuzzify crisp values to linguistic variables.
FuzzyMembership = NamedTuple(
    'FuzzyMembership',
    [('linguistic_variable', Enum),
     ('x1', float),
     ('x2', float)])

# Defines a fuzzy candle.
FuzzyCandle = NamedTuple(
    'FuzzyCandle',
    [('direction', CandleDirection),
     ('size', CandleSize),
     ('body_size', CandleBodySize),
     ('upper_wick_size', CandleWickSize),
     ('lower_wick_size', CandleWickSize)])


cdef class FuzzyCandlesticks(Indicator):
    """
    An indicator which fuzzifies bar data to produce fuzzy candlesticks.
    Bar data is dimensionally reduced via fuzzy feature extraction.
    """

    def __init__(self,
                 int period,
                 double threshold1,
                 double threshold2,
                 double threshold3,
                 double threshold4,
                 bint check_inputs=False):
        """
        Initializes a new instance of the FuzzyCandlesticks class.

        :param period: The rolling window period for the indicator (> 0).
        :param threshold1: The membership function x threshold1 (>= 0).
        :param threshold2: The membership function x threshold2 (> threshold1).
        :param threshold3: The membership function x threshold3 (> threshold2).
        :param threshold4: The membership function x threshold4 (> threshold3).
        :param check_inputs: The flag indicating whether the input values should be checked.
        """
        if self.check_inputs:
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
        self._value = None
        self._value_array = None
        self._value_price_comparison = None

    @property
    def value(self) -> FuzzyCandle:
        """
        The last FuzzyCandle.

        :return: The last fuzzy candle;
                [0] direction: CandleDirection
                [1] size: CandleSize
                [2] body_size: CandleBodySize
                [3] upper_wick_size: CandleWickSize
                [4] lower_wick_size: CandleWickSize
        """
        return self._value

    @property
    def value_array(self) -> np.array:
        """
        The last array representing the fuzzified features of the last bar.

        :return: The last fuzzy candle as a numpy array;
                 [0] direction: int (-1, 0, 1)
                 [1] size: int (0 to 6)
                 [2] body size: int (0 to 4)
                 [3] upper wick size: int (0 to 3)
                 [4] lower wick size: int (0 to 3)
        """
        return self._value_array

    @property
    def value_price_comparisons(self) -> np.array:
        """
        The last price comparisons (count_price_comparisons > 0).

        :return: The last price comparisons as a numpy array (1 higher, -1 lower, 0 equal);
                 [0] high vs last high
                 [1] low vs last low
                 [2] close vs last high
                 [3] close vs last low
                 [4] close vs last close
        """
        return self._value_price_comparison

    @cython.binding(True)
    cpdef void update(
            self,
            double open_price,
            double high_price,
            double low_price,
            double close_price):
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

        # Calculate price comparisons
        self._value_price_comparison = np.array(
            [self.price_comparison(high_price, self._last_high),
             self.price_comparison(low_price, self._last_low),
             self.price_comparison(close_price, self._last_high),
             self.price_comparison(close_price, self._last_low),
             self.price_comparison(close_price, self._last_close)])

        # Update last prices
        self._last_open = open_price
        self._last_high = high_price
        self._last_low = low_price
        self._last_close = close_price

        # Update measurements
        self._lengths.append(abs(high_price - low_price))

        if self._lengths[0] == 0.:
            self._body_percents.append(0.)
            self._upper_wick_percents.append(0.)
            self._lower_wick_percents.append(0.)
        else:
            self._body_percents.append(
                abs(open_price - low_price / self._lengths[0]))
            self._upper_wick_percents.append(
                (high_price - max(open_price, close_price)) / self._lengths[0])
            self._lower_wick_percents.append(
                (min(open_price, close_price) - low_price) / self._lengths[0])

        # Calculate statistics for bars
        cdef double mean_length = float(np.mean(self._lengths))
        cdef double mean_body_percent = float(np.mean(self._body_percents))
        cdef double mean_upper_wick = float(np.mean(self._upper_wick_percents))
        cdef double mean_lower_wick = float(np.mean(self._lower_wick_percents))

        cdef double sd_lengths = float(np.std(self._lengths))
        cdef double sd_body_percents = float(np.std(self._body_percents))
        cdef double sd_upper_wick_percents = float(np.std(self._upper_wick_percents))
        cdef double sd_lower_wick_percents = float(np.std(self._lower_wick_percents))

        # Create fuzzy candle
        self._value = FuzzyCandle(
            direction=FuzzyCandlesticks.fuzzify_direction(open_price, close_price),
            size=FuzzyCandlesticks._fuzzify_size(
                self, self._lengths[0],
                mean_length,
                sd_lengths),
            body_size=FuzzyCandlesticks._fuzzify_body_size(
                self,
                self._body_percents[0],
                mean_body_percent,
                sd_body_percents),
            upper_wick_size=FuzzyCandlesticks._fuzzify_wick_size(
                self,
                self._upper_wick_percents[0],
                mean_upper_wick,
                sd_upper_wick_percents),
            lower_wick_size=FuzzyCandlesticks._fuzzify_wick_size(
                self,
                self._lower_wick_percents[0],
                mean_lower_wick,
                sd_lower_wick_percents))

        # Create fuzzy candle as np array
        self._value_array = np.array(
            [int(self.value.direction.value),
             int(self.value.size.value),
             int(self.value.body_size.value),
             int(self.value.upper_wick_size.value),
             int(self.value.lower_wick_size.value)])

        # Initialization logic
        if self.initialized is False:
            self._set_has_inputs()
            if len(self._lengths) >= self.period:
                self._set_initialized()

    cpdef int price_comparison(self, double price1, double price2):
        """
        Compare the two given prices.

        :param price1: The first price to compare (> 0).
        :param price2: The second price to compare (> 0).
        :return: The result of the comparison (1 higher, -1 lower, 0 equal).
        """
        if price1 > price2:
            return 1
        if price1 < price2:
            return -1
        else:
            return 0

    @staticmethod
    def fuzzify_direction(double open_price, double close_price) -> CandleDirection:
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

    cdef object _fuzzify_size(
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
                x1=0.,
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
                return CandleSize(fuzzy_size.linguistic_variable)

    cdef object _fuzzify_body_size(
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
                x1=0.,
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
                return CandleBodySize(fuzzy_body_size.linguistic_variable)

    cdef object _fuzzify_wick_size(
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
                x1=0.,
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
                return CandleWickSize(fuzzy_wick_size.linguistic_variable)

    cpdef void reset(self):
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
        self._value = None
        self._value_array = None
        self._value_price_comparison = None
