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
from nautilus_trader.indicators.atr cimport AverageTrueRange
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.core.correctness cimport Condition


cdef class VolatilityCompressionRatio(Indicator):
    """
    An indicator which calculates the ratio of different ranges of volatility.
    Different moving average types can be selected for the inner ATR calculations.
    """

    def __init__(self,
                 int fast_period,
                 int slow_period,
                 ma_type not None: MovingAverageType=MovingAverageType.SIMPLE,
                 bint use_previous=True,
                 double value_floor=0.0,
                 bint check_inputs=False):
        """
        Initializes a new instance of the MovingAverageConvergenceDivergence class.

        :param fast_period: The period for the fast ATR (> 0).
        :param slow_period: The period for the slow ATR (> 0 & > fast_period).
        :param ma_type: The moving average type for the ATR calculations.
        :param use_previous: The boolean flag indicating whether previous price values should be used.
        :param value_floor: The floor (minimum) output value for the indicator (>= 0).
        :param check_inputs: The flag indicating whether the input values should be checked.
        """
        Condition.positive_int(fast_period, 'fast_period')
        Condition.positive_int(slow_period, 'slow_period')
        Condition.true(slow_period > fast_period, 'slow_period > fast_period')
        Condition.not_negative(value_floor, 'value_floor')

        super().__init__(params=[fast_period,
                                 slow_period,
                                 ma_type.name,
                                 use_previous,
                                 value_floor],
                         check_inputs=check_inputs)

        self._fast_period = fast_period
        self._slow_period = slow_period
        self._atr_fast = AverageTrueRange(fast_period, ma_type, use_previous, value_floor)
        self._atr_slow = AverageTrueRange(slow_period, ma_type, use_previous, value_floor)
        self.value = 0.0

    @cython.binding(True)
    cpdef void update(
            self,
            double high,
            double low,
            double close):
        """
        Update the indicator with the given values.

        :param high: The high price.
        :param low: The low price.
        :param close: The close price.
        """
        if self.check_inputs:
            Condition.positive(high, 'high')
            Condition.positive(low, 'low')
            Condition.positive(close, 'close')
            Condition.true(high >= low, 'high >= low')
            Condition.true(high >= close, 'high >= close')
            Condition.true(low <= close, 'low <= close')

        self._atr_fast.update(high, low, close)
        self._atr_slow.update(high, low, close)

        if self._atr_fast.value > 0.0:  # Guard against divide by zero
            self.value = self._atr_slow.value / self._atr_fast.value

        self._check_initialized()

    @cython.binding(True)
    cpdef void update_mid(self, double close):
        """
        Update the indicator with the given value.
        
        :param close: The close price.
        """
        if self.check_inputs:
            Condition.positive(close, 'close')

        self._atr_fast.update_mid(close)
        self._atr_slow.update_mid(close)

        if self._atr_fast.value > 0.0:  # Guard against divide by zero
            self.value = self._atr_slow.value / self._atr_fast.value

        self._check_initialized()

    cdef void _check_initialized(self):
        if not self.initialized:
            self._set_has_inputs()

            if self._atr_fast.initialized and self._atr_slow.initialized:
                self._set_initialized()

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._atr_fast.reset()
        self._atr_slow.reset()
        self.value = 0.0
