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
from statistics import mean

import numpy as np
import pandas as pd

from nautilus_trader.indicators.averages import MovingAverageFactory

cimport numpy as np
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.averages cimport MovingAverageType


cdef str get_ma_type_name(MovingAverageType ma_type):
    if ma_type == MovingAverageType.SIMPLE:
        return "SIMPLE"
    elif ma_type == MovingAverageType.EXPONENTIAL:
        return "EXPONENTIAL"
    elif ma_type == MovingAverageType.DOUBLE_EXPONENTIAL:
        return "DOUBLE_EXPONENTIAL"
    elif ma_type == MovingAverageType.WILDER:
        return "WILDER"
    elif ma_type == MovingAverageType.HULL:
        return "HULL"
    elif ma_type == MovingAverageType.ADAPTIVE:
        return "ADAPTIVE"
    elif ma_type == MovingAverageType.WEIGHTED:
        return "WEIGHTED"
    elif ma_type == MovingAverageType.VARIABLE_INDEX_DYNAMIC:
        return "VARIABLE_INDEX_DYNAMIC"
    else:
        return "UNKNOWN"
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.objects cimport Price


cdef class ArcherMovingAveragesTrends(Indicator):
    """
    Archer Moving Averages Trends indicator.

    Parameters
    ----------
    fast_period : int
        The period for the fast moving average (> 0).
    slow_period : int
        The period for the slow moving average (> 0 & > fast_sma).
    signal_period : int
        The period for lookback price array (> 0).
    ma_type : MovingAverageType
        The moving average type for the calculations.

    References
    ----------
    https://github.com/twopirllc/pandas-ta/blob/bc3b292bf1cc1d5f2aba50bb750a75209d655b37/pandas_ta/trend/amat.py
    """

    def __init__(
        self,
        int fast_period,
        int slow_period,
        int signal_period,
        MovingAverageType ma_type=MovingAverageType.EXPONENTIAL,
    ):
        Condition.positive_int(fast_period, "fast_period")
        Condition.positive_int(slow_period, "slow_period")
        Condition.is_true(slow_period > fast_period, "fast_period was >= slow_period")
        Condition.positive_int(signal_period, "signal_period")
        params = [
            fast_period,
            slow_period,
            signal_period,
            get_ma_type_name(ma_type),
        ]
        super().__init__(params=params)

        self.fast_period = fast_period
        self.slow_period = slow_period
        self.signal_period = signal_period
        self._fast_ma = MovingAverageFactory.create(fast_period, ma_type)
        self._slow_ma = MovingAverageFactory.create(slow_period, ma_type)
        self._fast_ma_price = deque(maxlen = signal_period + 1)
        self._slow_ma_price = deque(maxlen = signal_period + 1)
        self.long_run = 0
        self.short_run = 0

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
            bar.close.as_double(),
        )

    cpdef void update_raw(self, double close):
        """
        Update the indicator with the given close price value.

        Parameters
        ----------
        close : double
            The close price.

        """
        self._fast_ma.update_raw(close)
        self._slow_ma.update_raw(close)
        if self._slow_ma.initialized:
            self._fast_ma_price.append(self._fast_ma.value)
            self._slow_ma_price.append(self._slow_ma.value)

            self.long_run = (self._fast_ma_price[-1] - self._fast_ma_price[0] > 0 and \
                self._slow_ma_price[-1] - self._slow_ma_price[0] < 0 )
            self.long_run = (self._fast_ma_price[-1] - self._fast_ma_price[0] > 0 and \
                self._slow_ma_price[-1] - self._slow_ma_price[0] > 0 ) or self.long_run

            self.short_run = (self._fast_ma_price[-1] - self._fast_ma_price[0] < 0 and \
                self._slow_ma_price[-1] - self._slow_ma_price[0] > 0 )
            self.short_run = (self._fast_ma_price[-1] - self._fast_ma_price[0] < 0 and \
                self._slow_ma_price[-1] - self._slow_ma_price[0] < 0 ) or self.short_run

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._slow_ma_price) >= self.signal_period + 1 and self._slow_ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._fast_ma.reset()
        self._slow_ma.reset()
        self._fast_ma_price.clear()
        self._slow_ma_price.clear()
        self.long_run = 0
        self.short_run = 0


cdef class AroonOscillator(Indicator):
    """
    The Aroon (AR) indicator developed by Tushar Chande attempts to
    determine whether an instrument is trending, and how strong the trend is.
    AroonUp and AroonDown lines make up the indicator with their formulas below.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    """

    def __init__(self, int period):
        Condition.positive_int(period, "period")
        params = [
            period,
        ]
        super().__init__(params = params)

        self.period = period
        self._high_inputs = deque(maxlen = self.period + 1)
        self._low_inputs = deque(maxlen = self.period + 1)
        self.aroon_up = 0
        self.aroon_down = 0
        self.value = 0

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
        )

    cpdef void update_raw(
        self,
        double high,
        double low,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.
        """
        # Update inputs
        self._high_inputs.appendleft(high)
        self._low_inputs.appendleft(low)

        # Convert to double to compute values
        cdef double periods_from_hh = np.argmax(self._high_inputs)
        cdef double periods_from_ll = np.argmin(self._low_inputs)

        self.aroon_up = 100.0 * (1.0 - periods_from_hh / self.period)
        self.aroon_down = 100.0 * (1.0 - periods_from_ll / self.period)
        self.value = self.aroon_up - self.aroon_down

        self._check_initialized()

    cdef void _check_initialized(self):
        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._high_inputs) >= self.period + 1:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._high_inputs.clear()
        self._low_inputs.clear()
        self.aroon_up = 0
        self.aroon_down = 0
        self.value = 0


cdef class DirectionalMovement(Indicator):
    """
    Two oscillators that capture positive and negative trend movement.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the indicator (cannot be None).
    """

    def __init__(
        self,
        int period,
        MovingAverageType ma_type=MovingAverageType.EXPONENTIAL,
    ):
        Condition.positive_int(period, "period")
        params = [
            period,
            get_ma_type_name(ma_type),
        ]
        super().__init__(params=params)

        self.period = period
        self._pos_ma = MovingAverageFactory.create(period, ma_type)
        self._neg_ma = MovingAverageFactory.create(period, ma_type)
        self._previous_high = 0
        self._previous_low = 0
        self.pos = 0
        self.neg = 0

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
        )

    cpdef void update_raw(
        self,
        double high,
        double low,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.

        """
        if not self.has_inputs:
            self._previous_high = high
            self._previous_low = low

        cdef double up = high - self._previous_high
        cdef double dn = self._previous_low - low

        self._pos_ma.update_raw(((up > dn) and (up > 0)) * up)
        self._neg_ma.update_raw(((dn > up) and (dn > 0)) * dn)
        self.pos = self._pos_ma.value
        self.neg = self._neg_ma.value

        self._previous_high = high
        self._previous_low = low

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self._neg_ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        """
        Reset the indicator.

        All stateful fields are reset to their initial value.
        """
        self._pos_ma.reset()
        self._neg_ma.reset()
        self._previous_high = 0
        self._previous_low = 0
        self.pos = 0
        self.neg = 0


cdef class MovingAverageConvergenceDivergence(Indicator):
    """
    An indicator which calculates the difference between two moving averages.
    Different moving average types can be selected for the inner calculation.

    Parameters
    ----------
    fast_period : int
        The period for the fast moving average (> 0).
    slow_period : int
        The period for the slow moving average (> 0 & > fast_sma).
    ma_type : MovingAverageType
        The moving average type for the calculations.
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `fast_period` is not positive (> 0).
    ValueError
        If `slow_period` is not positive (> 0).
    ValueError
        If `fast_period` is not < `slow_period`.
    """

    def __init__(
        self,
        int fast_period,
        int slow_period,
        MovingAverageType ma_type=MovingAverageType.EXPONENTIAL,
        PriceType price_type=PriceType.LAST,
    ):
        Condition.positive_int(fast_period, "fast_period")
        Condition.positive_int(slow_period, "slow_period")
        Condition.is_true(slow_period > fast_period, "slow_period was <= fast_period")

        params=[
            fast_period,
            slow_period,
            get_ma_type_name(ma_type),
        ]
        super().__init__(params=params)

        self.fast_period = fast_period
        self.slow_period = slow_period
        self._fast_ma = MovingAverageFactory.create(fast_period, ma_type)
        self._slow_ma = MovingAverageFactory.create(slow_period, ma_type)
        self.price_type = price_type
        self.value = 0

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        cdef Price price = tick.extract_price(self.price_type)
        self.update_raw(Price.raw_to_f64_c(price._mem.raw))

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        self.update_raw(Price.raw_to_f64_c(tick._mem.price.raw))

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double close):
        """
        Update the indicator with the given close price.

        Parameters
        ----------
        close : double
            The close price.

        """
        self._fast_ma.update_raw(close)
        self._slow_ma.update_raw(close)
        self.value = self._fast_ma.value - self._slow_ma.value

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self._fast_ma.initialized and self._slow_ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._fast_ma.reset()
        self._slow_ma.reset()
        self.value = 0


cdef class LinearRegression(Indicator):
    """
    An indicator that calculates a simple linear regression.

    Parameters
    ----------
    period : int
        The period for the indicator.

    Raises
    ------
    ValueError
        If `period` is not greater than zero.
    """

    def __init__(self, int period=0):
        Condition.positive_int(period, "period")
        super().__init__(params=[period])

        self.period = period
        self._inputs = deque(maxlen=self.period)
        self.slope = 0.0
        self.intercept = 0.0
        self.degree = 0.0
        self.cfo = 0.0
        self.R2 = 0.0
        self.value = 0.0

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double close):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        close_price : double
            The close price.

        """
        self._inputs.append(close)

        # Warmup indicator logic
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._inputs) >= self.period:
                self._set_initialized(True)
            else:
                return

        cdef np.ndarray x_arr = np.arange(1, self.period + 1, dtype=np.float64)
        cdef np.ndarray y_arr = np.asarray(self._inputs, dtype=np.float64)
        cdef double x_sum = 0.5 * self.period * (self.period + 1)
        cdef double x2_sum = x_sum * (2 * self.period + 1) / 3
        cdef double divisor = self.period * x2_sum - x_sum * x_sum
        cdef double y_sum = sum(y_arr)
        cdef double xy_sum = sum(x_arr * y_arr)
        self.slope = (self.period * xy_sum - x_sum * y_sum) / divisor
        self.intercept = (y_sum * x2_sum - x_sum * xy_sum) / divisor

        cdef np.ndarray residuals = np.zeros(self.period, dtype=np.float64)
        cdef int i
        for i in np.arange(self.period):
            residuals[i] = self.slope * x_arr[i] + self.intercept - y_arr[i]

        self.value = residuals[-1] + y_arr[-1]
        self.degree = 180.0 / np.pi * np.arctan(self.slope)
        self.cfo = 100.0 * residuals[-1] / y_arr[-1]

        # Compute R2 with handling for zero variance in y_arr
        cdef double ssr = sum(residuals * residuals)
        cdef double sst = sum((y_arr - mean(y_arr)) * (y_arr - mean(y_arr)))
        if sst == 0.0:
            self.R2 = -np.inf
        else:
            self.R2 = 1.0 - ssr / sst

    cpdef void _reset(self):
        self._inputs.clear()
        self.slope = 0.0
        self.intercept = 0.0
        self.degree = 0.0
        self.cfo = 0.0
        self.R2 = 0.0
        self.value = 0.0


cdef class Bias(Indicator):
    """
    Rate of change between the source and a moving average.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the indicator (cannot be None).
    """

    def __init__(
        self,
        int period,
        MovingAverageType ma_type=MovingAverageType.SIMPLE,
    ):
        Condition.positive_int(period, "period")
        params = [
            period,
            get_ma_type_name(ma_type),
        ]
        super().__init__(params=params)

        self.period = period
        self._ma = MovingAverageFactory.create(period, ma_type)
        self.value = 0

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
            bar.close.as_double(),
        )

    cpdef void update_raw(self, double close):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        close : double
            The close price.

        """
        # Calculate average
        self._ma.update_raw(close)
        self.value = (close / self._ma.value) - 1.0
        self._check_initialized()

    cdef void _check_initialized(self):
        if not self.initialized:
            self._set_has_inputs(True)
            if self._ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._ma.reset()
        self.value = 0


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
