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

from nautilus_trader.indicators.averages import MovingAverageFactory

from libc.math cimport fabs

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


from nautilus_trader.core.stats cimport fast_std_with_mean
from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.objects cimport Price


cdef class AverageTrueRange(Indicator):
    """
    An indicator which calculates the average true range across a rolling window.
    Different moving average types can be selected for the inner calculation.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the indicator (cannot be None).
    use_previous : bool
        The boolean flag indicating whether previous price values should be used.
        (note: only applicable for `update()`. `update_mid()` will need to
        use previous price.
    value_floor : double
        The floor (minimum) output value for the indicator (>= 0).
    """

    def __init__(
        self,
        int period,
        MovingAverageType ma_type=MovingAverageType.SIMPLE,
        bint use_previous=True,
        double value_floor=0,
    ):
        Condition.positive_int(period, "period")
        Condition.not_negative(value_floor, "value_floor")
        params = [
            period,
            get_ma_type_name(ma_type),
            use_previous,
            value_floor,
        ]
        super().__init__(params=params)

        self.period = period
        self._ma = MovingAverageFactory.create(period, ma_type)
        self._use_previous = use_previous
        self._value_floor = value_floor
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

        self.update_raw(bar.high.as_double(), bar.low.as_double(), bar.close.as_double())

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
        # Calculate average
        if self._use_previous:
            if not self.has_inputs:
                self._previous_close = close
            self._ma.update_raw(max(self._previous_close, high) - min(low, self._previous_close))
            self._previous_close = close
        else:
            self._ma.update_raw(high - low)

        self._floor_value()
        self._check_initialized()

    cdef void _floor_value(self):
        if self._value_floor == 0:
            self.value = self._ma.value
        elif self._value_floor < self._ma.value:
            self.value = self._ma.value
        else:
            # Floor the value
            self.value = self._value_floor

    cdef void _check_initialized(self):
        if not self.initialized:
            self._set_has_inputs(True)
            if self._ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._ma.reset()
        self._previous_close = 0
        self.value = 0


cdef class BollingerBands(Indicator):
    """
    A Bollinger Band® is a technical analysis tool defined by a set of
    trend lines plotted two standard deviations (positively and negatively) away
    from a simple moving average (SMA) of an instruments price, which can be
    adjusted to user preferences.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    k : double
        The standard deviation multiple for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the indicator.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    ValueError
        If `k` is not positive (> 0).
    """

    def __init__(
        self,
        int period,
        double k,
        MovingAverageType ma_type=MovingAverageType.SIMPLE,
    ):
        Condition.positive_int(period, "period")
        Condition.positive(k, "k")
        super().__init__(params=[period, k, get_ma_type_name(ma_type)])

        self.period = period
        self.k = k
        self._ma = MovingAverageFactory.create(period, ma_type)
        self._prices = deque(maxlen=period)

        self.upper = 0.0
        self.middle = 0.0
        self.lower = 0.0

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given tick.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        cdef double bid = Price.raw_to_f64_c(tick._mem.bid_price.raw)
        cdef double ask = Price.raw_to_f64_c(tick._mem.ask_price.raw)
        cdef double mid = (ask + bid) / 2.0
        self.update_raw(ask, bid, mid)

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given tick.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        cdef double price = Price.raw_to_f64_c(tick._mem.price.raw)
        self.update_raw(price, price, price)

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

    cpdef void update_raw(self, double high, double low, double close):
        """
        Update the indicator with the given prices.

        Parameters
        ----------
        high : double
            The high price for calculations.
        low : double
            The low price for calculations.
        close : double
            The closing price for calculations

        """
        # Add data to queues
        cdef double typical = (high + low + close) / 3.0

        self._prices.append(typical)
        self._ma.update_raw(typical)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._prices) >= self.period:
                self._set_initialized(True)

        # Calculate values
        cdef double std = fast_std_with_mean(
            values=np.asarray(self._prices, dtype=np.float64),
            mean=self._ma.value,
        )

        # Set values
        self.upper = self._ma.value + (self.k * std)
        self.middle = self._ma.value
        self.lower = self._ma.value - (self.k * std)

    cpdef void _reset(self):
        self._ma.reset()
        self._prices.clear()

        self.upper = 0.0
        self.middle = 0.0
        self.lower = 0.0


cdef class DonchianChannel(Indicator):
    """
    Donchian Channels are three lines generated by moving average calculations
    that comprise an indicator formed by upper and lower bands around a
    mid-range or median band. The upper band marks the highest price of a
    instrument_id over N periods while the lower band marks the lowest price of a
    instrument_id over N periods. The area between the upper and lower bands
    represents the Donchian Channel.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    def __init__(self, int period):
        Condition.positive_int(period, "period")
        super().__init__(params=[period])

        self.period = period
        self._upper_prices = deque(maxlen=period)
        self._lower_prices = deque(maxlen=period)

        self.upper = 0
        self.middle = 0
        self.lower = 0

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given ticks high and low prices.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        cdef double ask = Price.raw_to_f64_c(tick._mem.ask_price.raw)
        cdef double bid = Price.raw_to_f64_c(tick._mem.bid_price.raw)
        self.update_raw(ask, bid)

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given ticks price.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        cdef double price = Price.raw_to_f64_c(tick._mem.price.raw)
        self.update_raw(price, price)

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.high.as_double(), bar.low.as_double())

    cpdef void update_raw(self, double high, double low):
        """
        Update the indicator with the given prices.

        Parameters
        ----------
        high : double
            The price for the upper channel.
        low : double
            The price for the lower channel.

        """
        # Add data to queues
        self._upper_prices.append(high)
        self._lower_prices.append(low)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._upper_prices) >= self.period and len(self._lower_prices) >= self.period:
                self._set_initialized(True)

        # Set values
        self.upper = max(self._upper_prices)
        self.lower = min(self._lower_prices)
        self.middle = (self.upper + self.lower) / 2

    cpdef void _reset(self):
        self._upper_prices.clear()
        self._lower_prices.clear()

        self.upper = 0
        self.middle = 0
        self.lower = 0


cdef class KeltnerChannel(Indicator):
    """
    The Keltner channel is a volatility based envelope set above and below a
    central moving average. Traditionally the middle band is an EMA based on the
    typical price (high + low + close) / 3, the upper band is the middle band
    plus the ATR. The lower band is the middle band minus the ATR.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    k_multiplier : double
        The multiplier for the ATR (> 0).
    ma_type : MovingAverageType
        The moving average type for the middle band (cannot be None).
    ma_type_atr : MovingAverageType
        The moving average type for the internal ATR (cannot be None).
    use_previous : bool
        The boolean flag indicating whether previous price values should be used.
    atr_floor : double
        The ATR floor (minimum) output value for the indicator (>= 0).
    """

    def __init__(
        self,
        int period,
        double k_multiplier,
        MovingAverageType ma_type=MovingAverageType.EXPONENTIAL,
        MovingAverageType ma_type_atr=MovingAverageType.SIMPLE,
        bint use_previous=True,
        double atr_floor=0,
    ):
        Condition.positive_int(period, "period")
        Condition.positive(k_multiplier, "k_multiplier")
        Condition.not_negative(atr_floor, "atr_floor")

        params = [
            period,
            k_multiplier,
            get_ma_type_name(ma_type),
            get_ma_type_name(ma_type_atr),
            use_previous,
            atr_floor,
        ]
        super().__init__(params=params)

        self.period = period
        self.k_multiplier = k_multiplier
        self._ma = MovingAverageFactory.create(period, ma_type)
        self._atr = AverageTrueRange(period, ma_type_atr, use_previous, atr_floor)
        self.upper = 0
        self.middle = 0
        self.lower = 0

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
            bar.close.as_double()
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

        self._ma.update_raw(typical_price)
        self._atr.update_raw(high, low, close)

        self.upper = self._ma.value + (self._atr.value * self.k_multiplier)
        self.middle = self._ma.value
        self.lower = self._ma.value - (self._atr.value * self.k_multiplier)

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
        self._ma.reset()
        self._atr.reset()
        self.upper = 0
        self.middle = 0
        self.lower = 0


cdef class VerticalHorizontalFilter(Indicator):
    """
    The Vertical Horizon Filter (VHF) was created by Adam White to identify
    trending and ranging markets.

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
        ]
        super().__init__(params=params)

        self.period = period
        self._prices = deque(maxlen=self.period)
        self._ma = MovingAverageFactory.create(period, ma_type)
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

        self.update_raw(
            bar.close.as_double(),
        )

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

        self._prices.append(close)

        cdef double max_price = max(self._prices)
        cdef double min_price = min(self._prices)

        self._ma.update_raw(fabs(close - self._previous_close))
        if self.initialized:
            self.value = fabs(max_price - min_price) / self.period / self._ma.value
        self._previous_close = close

        self._check_initialized()

    cdef void _check_initialized(self):
        if not self.initialized:
            self._set_has_inputs(True)
            if self._ma.initialized and len(self._prices) >= self.period:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._prices.clear()
        self._ma.reset()
        self._previous_close = 0
        self.value = 0


cdef class VolatilityRatio(Indicator):
    """
    An indicator which calculates the ratio of different ranges of volatility.
    Different moving average types can be selected for the inner ATR calculations.

    Parameters
    ----------
    fast_period : int
        The period for the fast ATR (> 0).
    slow_period : int
        The period for the slow ATR (> 0 & > fast_period).
    ma_type : MovingAverageType
        The moving average type for the ATR calculations.
    use_previous : bool
        The boolean flag indicating whether previous price values should be used.
    value_floor : double
        The floor (minimum) output value for the indicator (>= 0).

    Raises
    ------
    ValueError
        If `fast_period` is not positive (> 0).
    ValueError
        If `slow_period` is not positive (> 0).
    ValueError
        If `fast_period` is not < `slow_period`.
    ValueError
        If `value_floor` is negative (< 0).
    """

    def __init__(
        self,
        int fast_period,
        int slow_period,
        MovingAverageType ma_type=MovingAverageType.SIMPLE,
        bint use_previous=True,
        double value_floor=0,
    ):
        Condition.positive_int(fast_period, "fast_period")
        Condition.positive_int(slow_period, "slow_period")
        Condition.is_true(fast_period < slow_period, "fast_period was >= slow_period")
        Condition.not_negative(value_floor, "value_floor")

        params = [
            fast_period,
            slow_period,
            get_ma_type_name(ma_type),
            use_previous,
            value_floor,
        ]
        super().__init__(params=params)

        self.fast_period = fast_period
        self.slow_period = slow_period
        self._atr_fast = AverageTrueRange(fast_period, ma_type, use_previous, value_floor)
        self._atr_slow = AverageTrueRange(slow_period, ma_type, use_previous, value_floor)
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
            bar.close.as_double(),
        )

    cpdef void update_raw(
        self,
        double high,
        double low,
        double close,
    ):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.
        close : double
            The close price.

        """
        self._atr_fast.update_raw(high, low, close)
        self._atr_slow.update_raw(high, low, close)

        if self._atr_fast.value > 0:  # Guard against divide by zero
            self.value = self._atr_slow.value / self._atr_fast.value

        self._check_initialized()

    cdef void _check_initialized(self):
        if not self.initialized:
            self._set_has_inputs(True)

            if self._atr_fast.initialized and self._atr_slow.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._atr_fast.reset()
        self._atr_slow.reset()
        self.value = 0


cdef class KeltnerPosition(Indicator):
    """
    An indicator which calculates the relative position of the given price
    within a defined Keltner channel. This provides a measure of the relative
    'extension' of a market from the mean, as a multiple of volatility.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    k_multiplier : double
        The multiplier for the ATR (> 0).
    ma_type : MovingAverageType
        The moving average type for the middle band (cannot be None).
    ma_type_atr : MovingAverageType
        The moving average type for the internal ATR (cannot be None).
    use_previous : bool
        The boolean flag indicating whether previous price values should be used.
    atr_floor : double
        The ATR floor (minimum) output value for the indicator (>= 0).
    """

    def __init__(
        self,
        int period,
        double k_multiplier,
        MovingAverageType ma_type=MovingAverageType.EXPONENTIAL,
        MovingAverageType ma_type_atr=MovingAverageType.SIMPLE,
        bint use_previous=True,
        double atr_floor=0,
    ):
        Condition.positive_int(period, "period")
        Condition.positive(k_multiplier, "k_multiplier")
        Condition.not_negative(atr_floor, "atr_floor")

        params = [
            period,
            k_multiplier,
            get_ma_type_name(ma_type),
            get_ma_type_name(ma_type_atr),
            use_previous,
            atr_floor,
        ]
        super().__init__(params=params)

        self.period = period
        self.k_multiplier = k_multiplier

        self._kc = KeltnerChannel(
            period,
            k_multiplier,
            ma_type,
            ma_type_atr,
            use_previous,
            atr_floor,
        )

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
            bar.close.as_double(),
        )

    cpdef void update_raw(
        self,
        double high,
        double low,
        double close,
    ):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.
        close : double
            The close price.

        """
        self._kc.update_raw(high, low, close)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self._kc.initialized:
                self._set_initialized(True)

        cdef double k_width = (self._kc.upper - self._kc.lower) / 2

        if k_width > 0:
            self.value = (close - self._kc.middle) / k_width
        else:
            self.value = 0

    cpdef void _reset(self):
        self._kc.reset()
        self.value = 0
