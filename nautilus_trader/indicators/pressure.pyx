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

import cython

from nautilus_trader.indicators.average.moving_average import MovingAverageType
from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.atr cimport AverageTrueRange
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.core.correctness cimport Condition


cdef class Pressure(Indicator):
    """
    An indicator which calculates the relative volume (multiple of average volume)
    to move the market across a relative range (multiple of ATR).
    """

    def __init__(self,
                 int period,
                 ma_type not None: MovingAverageType=MovingAverageType.EXPONENTIAL,
                 double atr_floor=0.0,
                 bint check_inputs=False):
        """
        Initializes a new instance of the Pressure class.

        :param period: The period for the indicator (> 0).
        :param ma_type: The moving average type for the calculations.
        :param atr_floor: The ATR floor (minimum) output value for the indicator (>= 0.).
        :param check_inputs: The flag indicating whether the input values should be checked.
        """
        Condition.positive_int(period, 'period')
        Condition.not_negative(atr_floor, 'atr_floor')

        super().__init__(params=[period,
                                 ma_type.name,
                                 atr_floor],
                         check_inputs=check_inputs)
        self.period = period
        self._atr = AverageTrueRange(period, ma_type, atr_floor)
        self._average_volume = MovingAverageFactory.create(period, ma_type)
        self.value = 0.0
        self.value_cumulative = 0.0  # The sum of the pressure across the period

    @cython.binding(True)  # Needed for IndicatorUpdater to use this method as a delegate
    cpdef void update(
            self,
            double high,
            double low,
            double close,
            double volume) except *:
        """
        Update the indicator with the given values.

        :param high: The high price (> 0).
        :param low: The low price (> 0).
        :param close: The close price (> 0).
        :param volume: The volume (>= 0).
        """
        if self.check_inputs:
            Condition.positive(high, 'high')
            Condition.positive(low, 'low')
            Condition.positive(close, 'close')
            Condition.true(high >= low, 'high >= low')
            Condition.true(high >= close, 'high >= close')
            Condition.true(low <= close, 'low <= close')
            Condition.not_negative(volume, 'volume')

        self._atr.update(high, low, close)
        self._average_volume.update(volume)

        # Initialization logic (do not move this to the bottom as guard against zero will return)
        if not self.initialized:
            self._set_has_inputs(True)
            if self._atr.initialized:
                self._set_initialized(True)

        # Guard against zero values
        if self._average_volume.value == 0.0 or self._atr.value == 0.0:
            self.value = 0.0
            return

        cdef double relative_volume = volume / self._average_volume.value
        cdef double buy_pressure = ((close - low) / self._atr.value) * relative_volume
        cdef double sell_pressure = ((high - close) / self._atr.value) * relative_volume

        self.value = buy_pressure - sell_pressure
        self.value_cumulative += self.value

    cpdef void reset(self) except *:
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._atr.reset()
        self._average_volume.reset()
        self.value = 0.0
        self.value_cumulative = 0.0
