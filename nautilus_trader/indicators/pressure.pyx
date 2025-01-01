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

from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.average.moving_average import MovingAverageType

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.atr cimport AverageTrueRange
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


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
        ma_type not None: MovingAverageType=MovingAverageType.EXPONENTIAL,
        double atr_floor=0,
    ):
        Condition.positive_int(period, "period")
        Condition.not_negative(atr_floor, "atr_floor")

        params=[
            period,
            ma_type.name,
            atr_floor,
        ]
        super().__init__(params=params)

        self.period = period
        self._atr = AverageTrueRange(period, MovingAverageType.EXPONENTIAL, atr_floor)
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
