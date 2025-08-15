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
from math import log

import numpy as np

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.stats cimport fast_mad_with_mean
from nautilus_trader.core.stats cimport fast_std_with_mean
from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef str get_ma_type_name(ma_type):
    """Helper function to get MovingAverageType name for params."""
    from nautilus_trader.indicators.averages import MovingAverageType

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


cdef class RelativeStrengthIndex(Indicator):
    """
    An indicator which calculates a relative strength index (RSI) across a rolling window.

    Parameters
    ----------
    ma_type : int
        The moving average type for average gain/loss.
    period : MovingAverageType
        The rolling window period for the indicator.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    def __init__(
        self,
        int period,
        ma_type=None,
    ):
        from nautilus_trader.indicators.averages import MovingAverageFactory
        from nautilus_trader.indicators.averages import MovingAverageType

        if ma_type is None:
            ma_type = MovingAverageType.EXPONENTIAL
        Condition.positive_int(period, "period")
        super().__init__(params=[period, get_ma_type_name(ma_type)])

        self.period = period
        self._rsi_max = 1
        self._average_gain = MovingAverageFactory.create(period, ma_type)
        self._average_loss = MovingAverageFactory.create(period, ma_type)
        self._last_value = 0
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

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value):
        """
        Update the indicator with the given value.

        Parameters
        ----------
        value : double
            The update value.

        """
        # Check if first input
        if not self.has_inputs:
            self._last_value = value
            self._set_has_inputs(True)

        cdef double gain = value - self._last_value

        if gain > 0:
            self._average_gain.update_raw(gain)
            self._average_loss.update_raw(0)
        elif gain < 0:
            self._average_loss.update_raw(-gain)
            self._average_gain.update_raw(0)
        else:
            self._average_gain.update_raw(0)
            self._average_loss.update_raw(0)

        # Initialization logic
        if not self.initialized:
            if self._average_gain.initialized and self._average_loss.initialized:
                self._set_initialized(True)

        if self._average_loss.value == 0:
            self.value = self._rsi_max
            self._last_value = value
            return

        cdef double rs = self._average_gain.value / self._average_loss.value

        self.value = self._rsi_max - (self._rsi_max / (1 + rs))
        self._last_value = value

    cpdef void _reset(self):
        self._average_gain.reset()
        self._average_loss.reset()
        self._last_value = 0
        self.value = 0


cdef class RateOfChange(Indicator):
    """
    An indicator which calculates the rate of change of price over a defined period.
    The return output can be simple or log.

    Parameters
    ----------
    period : int
        The period for the indicator.
    use_log : bool
        Use log returns for value calculation.

    Raises
    ------
    ValueError
        If `period` is not > 1.
    """

    def __init__(self, int period, bint use_log=False):
        Condition.is_true(period > 1, "period was <= 1")
        super().__init__(params=[period])

        self.period = period
        self._use_log = use_log
        self._prices = deque(maxlen=period)
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

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double price):
        """
        Update the indicator with the given price.

        Parameters
        ----------
        price : double
            The update price.

        """
        self._prices.append(price)

        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._prices) >= self.period:
                self._set_initialized(True)

        if self._use_log:
            self.value = log(price / self._prices[0])
        else:
            self.value = (price - self._prices[0]) / self._prices[0]

    cpdef void _reset(self):
        self._prices.clear()
        self.value = 0


cdef class ChandeMomentumOscillator(Indicator):
    """
    Attempts to capture the momentum of an asset with overbought at 50 and
    oversold at -50.

    Parameters
    ----------
    ma_type : int
        The moving average type for average gain/loss.
    period : MovingAverageType
        The rolling window period for the indicator.
    """

    def __init__(
        self,
        int period,
        ma_type=None,
    ):
        from nautilus_trader.indicators.averages import MovingAverageFactory
        from nautilus_trader.indicators.averages import MovingAverageType

        if ma_type is None:
            ma_type = MovingAverageType.WILDER
        params = [
            period,
            get_ma_type_name(ma_type),
        ]
        super().__init__(params = params)

        self.period = period
        self._average_gain = MovingAverageFactory.create(period, ma_type)
        self._average_loss = MovingAverageFactory.create(period, ma_type)
        self._previous_close = 0
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

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double close):
        """
        Update the indicator with the given value.

        Parameters
        ----------
        value : double
            The update value.

        """
        # Check if first input
        if not self.has_inputs:
            self._set_has_inputs(True)
            self._previous_close = close

        cdef double gain = close - self._previous_close

        if gain > 0:
            self._average_gain.update_raw(gain)
            self._average_loss.update_raw(0)
        elif gain < 0:
            self._average_gain.update_raw(0)
            self._average_loss.update_raw(-gain)
        else:
            self._average_gain.update_raw(0)
            self._average_loss.update_raw(0)
        # Initialization logic
        if not self.initialized:
            if self._average_gain.initialized and self._average_loss.initialized:
                self._set_initialized(True)

        cdef double divisor
        if self.initialized:
            divisor = self._average_gain.value + self._average_loss.value
            if divisor == 0.0:
                self.value = 0.0
            else:
                self.value = 100.0 * (self._average_gain.value - self._average_loss.value) / divisor

        self._previous_close = close

    cpdef void _reset(self):
        self._average_gain.reset()
        self._average_loss.reset()
        self._previous_close = 0
        self.value = 0


cdef class Stochastics(Indicator):
    """
    An oscillator which can indicate when an asset may be over bought or over
    sold.

    Parameters
    ----------
    period_k : int
        The period for the K line.
    period_d : int
        The period for the D line.

    Raises
    ------
    ValueError
        If `period_k` is not positive (> 0).
    ValueError
        If `period_d` is not positive (> 0).

    References
    ----------
    https://www.forextraders.com/forex-education/forex-indicators/stochastics-indicator-explained/
    """

    def __init__(self, int period_k, int period_d):
        Condition.positive_int(period_k, "period_k")
        Condition.positive_int(period_d, "period_d")
        super().__init__(params=[period_k, period_d])

        self.period_k = period_k
        self.period_d = period_d
        self._highs = deque(maxlen=period_k)
        self._lows = deque(maxlen=period_k)
        self._c_sub_l = deque(maxlen=period_d)
        self._h_sub_l = deque(maxlen=period_d)

        self.value_k = 0
        self.value_d = 0

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
            bar.close.as_double(),
        )

    cpdef void update_raw(
        self,
        double high,
        double low,
        double close,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.
        close : double
            The close price.

        """
        # Check if first input
        if not self.has_inputs:
            self._set_has_inputs(True)

        self._highs.append(high)
        self._lows.append(low)

        # Initialization logic
        if not self.initialized:
            if len(self._highs) == self.period_k and len(self._lows) == self.period_k:
                self._set_initialized(True)

        cdef double k_max_high = max(self._highs)
        cdef double k_min_low = min(self._lows)

        self._c_sub_l.append(close - k_min_low)
        self._h_sub_l.append(k_max_high - k_min_low)

        if k_max_high == k_min_low:
            return  # Divide by zero guard

        self.value_k = 100 * ((close - k_min_low) / (k_max_high - k_min_low))
        self.value_d = 100 * (sum(self._c_sub_l) / sum(self._h_sub_l))

    cpdef void _reset(self):
        self._highs.clear()
        self._lows.clear()
        self._c_sub_l.clear()
        self._h_sub_l.clear()

        self.value_k = 0
        self.value_d = 0


cdef class CommodityChannelIndex(Indicator):
    """
    Commodity Channel Index is a momentum oscillator used to primarily identify
    overbought and oversold levels relative to a mean.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    scalar : double
        A positive float to scale the bands
    ma_type : MovingAverageType
        The moving average type for prices.

    References
    ----------
    https://www.tradingview.com/support/solutions/43000502001-commodity-channel-index-cci/
    """

    def __init__(
        self,
        int period,
        double scalar = 0.015,
        ma_type=None,
    ):
        from nautilus_trader.indicators.averages import MovingAverageFactory
        from nautilus_trader.indicators.averages import MovingAverageType

        if ma_type is None:
            ma_type = MovingAverageType.SIMPLE
        Condition.positive_int(period, "period")

        params = [
            period,
            scalar,
            get_ma_type_name(ma_type),
        ]
        super().__init__(params=params)

        self.period = period
        self.scalar = scalar
        self._prices = deque(maxlen=period)
        self._ma = MovingAverageFactory.create(period, ma_type)
        self._mad = 0.0
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

        self.update_raw(
            bar.high.as_double(),
            bar.low.as_double(),
            bar.close.as_double(),
        )

    cpdef void update_raw(
        self,
        double high,
        double low,
        double close,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.
        close : double
            The close price.

        """
        cdef double typical_price = (high + low + close) / 3.0
        self._prices.append(typical_price)
        self._ma.update_raw(typical_price)
        self._mad = fast_mad_with_mean(
            values=np.asarray(self._prices, dtype=np.float64),
            mean=self._ma.value,
        )
        if self._ma.initialized:
            self.value = (typical_price - self._ma.value) / (self.scalar * self._mad)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self._ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        """
        Reset the indicator.

        All stateful fields are reset to their initial value.
        """
        self._prices.clear()
        self._ma.reset()
        self._mad = 0.0
        self.value = 0.0


cdef class EfficiencyRatio(Indicator):
    """
    An indicator which calculates the efficiency ratio across a rolling window.
    The Kaufman Efficiency measures the ratio of the relative market speed in
    relation to the volatility, this could be thought of as a proxy for noise.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (>= 2).

    Raises
    ------
    ValueError
        If `period` is not >= 2.
    """

    def __init__(self, int period):
        Condition.is_true(period >= 2, "period was < 2")
        super().__init__(params=[period])

        self.period = period
        self._inputs = deque(maxlen=period)
        self._deltas = deque(maxlen=period)
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

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double price):
        """
        Update the indicator with the given price.

        Parameters
        ----------
        price : double
            The update price.

        """
        self._inputs.append(price)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._inputs) < 2:
                return  # Not enough data
            elif len(self._inputs) >= self.period:
                self._set_initialized(True)

        # Add data to queues
        self._deltas.append(abs(self._inputs[-1] - self._inputs[-2]))

        # Calculate efficiency ratio
        cdef double net_diff = abs(self._inputs[0] - self._inputs[-1])
        cdef double sum_deltas = sum(self._deltas)

        if sum_deltas > 0:
            self.value = net_diff / sum_deltas
        else:
            self.value = 0

    cpdef void _reset(self):
        self._inputs.clear()
        self._deltas.clear()
        self.value = 0


cdef class RelativeVolatilityIndex(Indicator):
    """
    The Relative Volatility Index (RVI) was created in 1993 and revised in 1995.
    Instead of adding up price changes like RSI based on price direction, the RVI
    adds up standard deviations based on price direction.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    scalar : double
        A positive float to scale the bands.
    ma_type : MovingAverageType
        The moving average type for the vip and vim (cannot be None).
    """

    def __init__(
        self,
        int period,
        double scalar = 100.0,
        ma_type=None,
    ):
        from nautilus_trader.indicators.averages import MovingAverageFactory
        from nautilus_trader.indicators.averages import MovingAverageType

        if ma_type is None:
            ma_type = MovingAverageType.EXPONENTIAL
        Condition.positive_int(period, "period")

        params = [
            period,
            scalar,
            get_ma_type_name(ma_type),
        ]
        super().__init__(params=params)

        self.period = period
        self.scalar = scalar
        self._prices = deque(maxlen=period)
        self._ma = MovingAverageFactory.create(period, MovingAverageType.SIMPLE)
        self._pos_ma = MovingAverageFactory.create(period, ma_type)
        self._neg_ma = MovingAverageFactory.create(period, ma_type)
        self._previous_close = 0
        self._std = 0
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

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double close):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        close : double
            The close price.

        """
        self._prices.append(close)
        self._ma.update_raw(close)

        self._std = fast_std_with_mean(
            values=np.asarray(self._prices, dtype=np.float64),
            mean=self._ma.value,
        )

        self._std = self._std * np.sqrt(self.period) / np.sqrt(self.period - 1)

        if self._ma.initialized:
            if close > self._previous_close:
                self._pos_ma.update_raw(self._std)
                self._neg_ma.update_raw(0)
            elif close < self._previous_close:
                self._pos_ma.update_raw(0)
                self._neg_ma.update_raw(self._std)
            else:
                self._pos_ma.update_raw(0)
                self._neg_ma.update_raw(0)

            self.value = self.scalar * self._pos_ma.value
            self.value = self.value / (self._pos_ma.value + self._neg_ma.value)


        self._previous_close = close

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if  self._pos_ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        """
        Reset the indicator.

        All stateful fields are reset to their initial value.
        """
        self._prices.clear()
        self._ma.reset()
        self._pos_ma.reset()
        self._neg_ma.reset()
        self._previous_close = 0
        self._std = 0
        self.value = 0


cdef class PsychologicalLine(Indicator):
    """
    The Psychological Line is an oscillator-type indicator that compares the
    number of the rising periods to the total number of periods. In other
    words, it is the percentage of bars that close above the previous
    bar over a given period.

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
        ma_type=None,
    ):
        from nautilus_trader.indicators.averages import MovingAverageFactory
        from nautilus_trader.indicators.averages import MovingAverageType

        if ma_type is None:
            ma_type = MovingAverageType.SIMPLE
        Condition.positive_int(period, "period")
        params = [
            period,
            get_ma_type_name(ma_type),
        ]
        super().__init__(params=params)

        self.period = period
        self._ma = MovingAverageFactory.create(period, ma_type)
        self._diff = 0
        self._previous_close = 0
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

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double close):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        close : double
            The close price.

        """
        # Update inputs
        if not self.has_inputs:
            self._previous_close = close

        self._diff = close - self._previous_close
        if self._diff <= 0:
            self._ma.update_raw(0)
        else:
            self._ma.update_raw(1)
        self.value = 100.0 * self._ma.value

        if not self.initialized:
            self._set_has_inputs(True)
            if self._ma.initialized:
                self._set_initialized(True)
        self._previous_close = close

    cpdef void _reset(self):
        self._ma.reset()
        self._diff = 0
        self._previous_close = 0
        self.value = 0
