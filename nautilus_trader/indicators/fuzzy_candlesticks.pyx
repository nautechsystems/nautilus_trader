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

import numpy as np

cimport numpy as np
from libc.math cimport fabs

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.stats cimport fast_mean
from nautilus_trader.core.stats cimport fast_std_with_mean
from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.indicators.fuzzy_enums cimport CandleBodySize
from nautilus_trader.indicators.fuzzy_enums cimport CandleDirection
from nautilus_trader.indicators.fuzzy_enums cimport CandleSize
from nautilus_trader.indicators.fuzzy_enums cimport CandleWickSize
from nautilus_trader.model.data cimport Bar


cdef class FuzzyCandle:
    """
    Represents a fuzzy candle.

    Parameters
    ----------
    direction : CandleDirection
        The candle direction.
    size : CandleSize
        The candle fuzzy size.
    body_size : CandleBodySize
        The candle fuzzy body size.
    upper_wick_size : CandleWickSize
        The candle fuzzy upper wick size.
    lower_wick_size : CandleWickSize
        The candle fuzzy lower wick size.
    """

    def __init__(
        self,
        CandleDirection direction,
        CandleSize size,
        CandleBodySize body_size,
        CandleWickSize upper_wick_size,
        CandleWickSize lower_wick_size,
    ):
        self.direction = direction
        self.size = size
        self.body_size = body_size
        self.upper_wick_size = upper_wick_size
        self.lower_wick_size = lower_wick_size

    def __eq__(self, FuzzyCandle other) -> bool:
        return self.direction == other.direction \
            and self.size == other.size \
            and self.body_size == other.body_size \
            and self.upper_wick_size == other.upper_wick_size \
            and self.lower_wick_size == other.lower_wick_size

    def __str__(self) -> str:
        return f"({self.direction}, {self.size}, {self.body_size}, {self.lower_wick_size}, {self.upper_wick_size})"

    def __repr__(self) -> str:
        return f"{self.__class__.__name__}{self}"


cdef class FuzzyCandlesticks(Indicator):
    """
    An indicator which fuzzifies bar data to produce fuzzy candlesticks.
    Bar data is dimensionally reduced via fuzzy feature extraction.

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
    """

    def __init__(
        self,
        int period,
        double threshold1=0.5,
        double threshold2=1.0,
        double threshold3=2.0,
        double threshold4=3.0,
    ):
        Condition.positive_int(period, "period")
        Condition.positive(threshold1, "threshold1")
        Condition.is_true(threshold2 > threshold1, "threshold2 was <= threshold1")
        Condition.is_true(threshold3 > threshold2, "threshold3 was <= threshold2")
        Condition.is_true(threshold4 > threshold3, "threshold4 was <= threshold3")
        super().__init__(
            params=[
                period,
                threshold1,
                threshold2,
                threshold3,
                threshold4,
            ]
        )

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

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        The update bar.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(
            bar.open.as_double(),
            bar.high.as_double(),
            bar.low.as_double(),
            bar.close.as_double(),
        )

    cpdef void update_raw(
        self,
        double open,
        double high,
        double low,
        double close,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        open : double
            The open price.
        high : double
            The high price.
        low : double
            The low price.
        close : double
            The close price.

        """

        # Update last prices
        self._last_open = open
        self._last_high = high
        self._last_low = low
        self._last_close = close

        # Update measurements
        cdef double length = fabs(high - low)
        self._lengths.append(length)

        if length == 0.0:
            self._body_percents.append(0.0)
            self._upper_wick_percents.append(0.0)
            self._lower_wick_percents.append(0.0)
        else:
            self._body_percents.append(fabs(open - close) / length)
            self._upper_wick_percents.append((high - max(open, close)) / length)
            self._lower_wick_percents.append((min(open, close) - low) / length)

        cdef np.ndarray lengths = np.asarray(self._lengths, dtype=np.float64)
        cdef np.ndarray body_percents = np.asarray(self._body_percents, dtype=np.float64)
        cdef np.ndarray upper_wick_percents = np.asarray(self._upper_wick_percents, dtype=np.float64)
        cdef np.ndarray lower_wick_percents = np.asarray(self._lower_wick_percents, dtype=np.float64)

        # Calculate statistics for bars
        cdef double mean_length = fast_mean(lengths)
        cdef double mean_body_percent = fast_mean(body_percents)
        cdef double mean_upper_wick = fast_mean(upper_wick_percents)
        cdef double mean_lower_wick = fast_mean(lower_wick_percents)

        cdef double sd_lengths = fast_std_with_mean(lengths, mean_length)
        cdef double sd_body_percents = fast_std_with_mean(body_percents, mean_body_percent)
        cdef double sd_upper_wick_percents = fast_std_with_mean(upper_wick_percents, mean_upper_wick)
        cdef double sd_lower_wick_percents = fast_std_with_mean(lower_wick_percents, mean_lower_wick)

        # Create fuzzy candle
        self.value = FuzzyCandle(
            direction=self._fuzzify_direction(open, close),
            size=self._fuzzify_size(
                length,
                mean_length,
                sd_lengths),
            body_size=self._fuzzify_body_size(
                self._body_percents[-1],
                mean_body_percent,
                sd_body_percents),
            upper_wick_size=self._fuzzify_wick_size(
                self._upper_wick_percents[-1],
                mean_upper_wick,
                sd_upper_wick_percents),
            lower_wick_size=self._fuzzify_wick_size(
                self._lower_wick_percents[-1],
                mean_lower_wick,
                sd_lower_wick_percents),
        )

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

    cdef CandleDirection _fuzzify_direction(self, double open, double close):
        # Fuzzify the candle entry from the given inputs
        if close > open:
            return CandleDirection.DIRECTION_BULL
        if close < open:
            return CandleDirection.DIRECTION_BEAR
        else:
            return CandleDirection.DIRECTION_NONE

    cdef CandleSize _fuzzify_size(
            self,
            double length,
            double mean_length,
            double sd_lengths):
        # Fuzzify the candle size from the given inputs
        if length == 0:
            return CandleSize.SIZE_NONE

        cdef double x

        # Determine CandleSize fuzzy membership
        # -------------------------------------
        # CandleSize.VERY_SMALL
        x = mean_length - (sd_lengths * self._threshold2)
        if length <= x:
            return CandleSize.SIZE_VERY_SMALL

        # CandleSize.SMALL
        x = mean_length + (sd_lengths * self._threshold1)
        if length <= x:
            return CandleSize.SIZE_SMALL

        # CandleSize.MEDIUM
        x = mean_length + sd_lengths * self._threshold2
        if length <= x:
            return CandleSize.SIZE_MEDIUM

        # CandleSize.LARGE
        x = mean_length + (sd_lengths * self._threshold3)
        if length <= x:
            return CandleSize.SIZE_LARGE

        # CandleSize.VERY_LARGE
        x = mean_length + (sd_lengths * self._threshold4)
        if length <= x:
            return CandleSize.SIZE_VERY_LARGE

        return CandleSize.SIZE_EXTREMELY_LARGE

    cdef CandleBodySize _fuzzify_body_size(
            self,
            double body_percent,
            double mean_body_percent,
            double sd_body_percents):
        # Fuzzify the candle body size from the given inputs
        if body_percent == 0:
            return CandleBodySize.BODY_NONE

        cdef double x

        # Determine CandleBodySize fuzzy membership
        # -----------------------------------------
        # CandleBodySize.SMALL
        x = mean_body_percent - (sd_body_percents * self._threshold1)
        if body_percent <= x:
            return CandleBodySize.BODY_SMALL

        # CandleBodySize.MEDIUM
        x = mean_body_percent + (sd_body_percents * self._threshold1)
        if body_percent <= x:
            return CandleBodySize.BODY_MEDIUM

        # CandleBodySize.LARGE
        x = mean_body_percent + (sd_body_percents * self._threshold2)
        if body_percent <= x:
            return CandleBodySize.BODY_LARGE

        return CandleBodySize.BODY_TREND

    cdef CandleWickSize _fuzzify_wick_size(
            self,
            double wick_percent,
            double mean_wick_percent,
            double sd_wick_percents):
        # Fuzzify the candle wick size from the given inputs
        if wick_percent == 0:
            return CandleWickSize.WICK_NONE

        cdef double x

        # Determine CandleWickSize fuzzy membership
        # -----------------------------------------
        # CandleWickSize.SMALL
        x = mean_wick_percent - (sd_wick_percents * self._threshold1)
        if wick_percent <= x:
            return CandleWickSize.WICK_SMALL

        # CandleWickSize.MEDIUM
        x = mean_wick_percent + (sd_wick_percents * self._threshold2)
        if wick_percent <= x:
            return CandleWickSize.WICK_MEDIUM

        return CandleWickSize.WICK_LARGE

    cpdef void _reset(self):
        self._lengths.clear()
        self._body_percents.clear()
        self._upper_wick_percents.clear()
        self._lower_wick_percents.clear()
        self._last_open = 0
        self._last_high = 0
        self._last_low = 0
        self._last_close = 0
        self.vector = None
        self.value = None
