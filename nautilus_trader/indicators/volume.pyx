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

from nautilus_trader.indicators.averages import MovingAverageFactory
from nautilus_trader.indicators.volatility import AverageTrueRange

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.averages cimport MovingAverageType
from nautilus_trader.indicators.base cimport Indicator
from nautilus_trader.model.data cimport Bar


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


cdef class OnBalanceVolume(Indicator):
    """
    An indicator which calculates the momentum of relative positive or negative
    volume.

    Parameters
    ----------
    period : int
        The period for the indicator, zero indicates no window (>= 0).

    Raises
    ------
    ValueError
        If `period` is negative (< 0).
    """

    def __init__(self, int period=0):
        Condition.not_negative(period, "period")
        super().__init__(params=[period])

        self.period = period
        self._obv = deque(maxlen=None if period == 0 else period)
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
            bar.open.as_double(),
            bar.close.as_double(),
            bar.volume.as_double(),
        )

    cpdef void update_raw(
        self,
        double open,
        double close,
        double volume,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        open : double
            The high price.
        close : double
            The low price.
        volume : double
            The close price.

        """
        if close > open:
            self._obv.append(volume)
        elif close < open:
            self._obv.append(-volume)
        else:
            self._obv.append(0)

        self.value = sum(self._obv)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if (self.period == 0 and len(self._obv) > 0) or len(self._obv) >= self.period:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._obv.clear()
        self.value = 0


cdef class VolumeWeightedAveragePrice(Indicator):
    """
    An indicator which calculates the volume weighted average price for the day.
    """

    def __init__(self):
        super().__init__(params=[])

        self._day = 0
        self._price_volume = 0
        self._volume_total = 0
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
            (
                bar.close.as_double() +
                bar.high.as_double() +
                bar.low.as_double()
            ) / 3.0,
            bar.volume.as_double(),
            pd.Timestamp(bar.ts_init, tz="UTC"),
        )

    cpdef void update_raw(
        self,
        double price,
        double volume,
        datetime timestamp,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        price : double
            The update price.
        volume : double
            The update volume.
        timestamp : datetime
            The current timestamp.

        """
        # On a new day reset the indicator
        if timestamp.day != self._day:
            self.reset()
            self._day = timestamp.day
            self.value = price

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            self._set_initialized(True)

        # No weighting for this price (also avoiding divide by zero)
        if volume == 0:
            return

        self._price_volume += price * volume
        self._volume_total += volume
        self.value = self._price_volume / self._volume_total

    cpdef void _reset(self):
        self._day = 0
        self._price_volume = 0
        self._volume_total = 0
        self.value = 0


cdef class KlingerVolumeOscillator(Indicator):
    """
    This indicator was developed by Stephen J. Klinger. It is designed to predict
    price reversals in a market by comparing volume to price.

    Parameters
    ----------
    fast_period : int
        The period for the fast moving average (> 0).
    slow_period : int
        The period for the slow moving average (> 0 & > fast_sma).
    signal_period : int
        The period for the moving average difference's moving average (> 0).
    ma_type : MovingAverageType
        The moving average type for the calculations.
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
        self._signal_ma = MovingAverageFactory.create(signal_period, ma_type)
        self._hlc3 = 0
        self._previous_hlc3 = 0
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
            bar.volume.as_double(),
        )

    cpdef void update_raw(
        self,
        double high,
        double low,
        double close,
        double volume,
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
        volume : double
            The volume.

        """
        self._hlc3 = (high + low + close)/3.0

        if self._hlc3 > self._previous_hlc3:
            self._fast_ma.update_raw(volume)
            self._slow_ma.update_raw(volume)
        elif self._hlc3 < self._previous_hlc3:
            self._fast_ma.update_raw(-volume)
            self._slow_ma.update_raw(-volume)
        else:
            self._fast_ma.update_raw(0)
            self._slow_ma.update_raw(0)

        if self._slow_ma.initialized:
            self._signal_ma.update_raw(self._fast_ma.value - self._slow_ma.value)
            self.value = self._signal_ma.value

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self._signal_ma.initialized:
                self._set_initialized(True)

        self._previous_hlc3 = self._hlc3

    cpdef void _reset(self):
        self._fast_ma.reset()
        self._slow_ma.reset()
        self._signal_ma.reset()
        self._hlc3 = 0
        self._previous_hlc3 = 0
        self.value = 0


cdef class Pressure(Indicator):
    """
    An indicator which calculates the relative volume (multiple of average volume)
    to move the market across a relative range (multiple of ATR).

    Parameters
    ----------
    period : int
        The period for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the calculations.
    atr_floor : double
        The ATR floor (minimum) output value for the indicator (>= 0.).

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    ValueError
        If `atr_floor` is negative (< 0).
    """

    def __init__(
        self,
        int period,
        MovingAverageType ma_type=MovingAverageType.EXPONENTIAL,
        double atr_floor=0,
    ):
        Condition.positive_int(period, "period")
        Condition.not_negative(atr_floor, "atr_floor")

        params=[
            period,
            get_ma_type_name(ma_type),
            atr_floor,
        ]
        super().__init__(params=params)

        self.period = period
        self._atr = AverageTrueRange(period, ma_type, True, atr_floor)
        self._average_volume = MovingAverageFactory.create(period, ma_type)
        self.value = 0
        self.value_cumulative = 0

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
            bar.volume.as_double(),
        )

    cpdef void update_raw(
        self,
        double high,
        double low,
        double close,
        double volume,
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
        volume : double
            The volume.

        """
        self._atr.update_raw(high, low, close)
        self._average_volume.update_raw(volume)

        # Initialization logic (do not move this to the bottom as guard against zero will return)
        if not self.initialized:
            self._set_has_inputs(True)
            if self._atr.initialized:
                self._set_initialized(True)

        # Guard against zero values
        if self._average_volume.value == 0 or self._atr.value == 0:
            self.value = 0
            return

        cdef double relative_volume = volume / self._average_volume.value
        cdef double buy_pressure = ((close - low) / self._atr.value) * relative_volume
        cdef double sell_pressure = ((high - close) / self._atr.value) * relative_volume

        self.value = buy_pressure - sell_pressure
        self.value_cumulative += self.value

    cpdef void _reset(self):
        self._atr.reset()
        self._average_volume.reset()
        self.value = 0
        self.value_cumulative = 0
