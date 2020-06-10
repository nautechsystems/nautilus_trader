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
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.core.correctness cimport Condition


cdef class RelativeStrengthIndex(Indicator):
    """
    An indicator which calculates a relative strength index (RSI) across a rolling window.
    """

    def __init__(self,
                 int period,
                 ma_type not None: MovingAverageType=MovingAverageType.EXPONENTIAL):
        """
        Initializes a new instance of the RelativeStrengthIndex class.

        :param ma_type: The moving average type for average gain/loss.
        :param period: The rolling window period for the indicator (> 0).
        """
        Condition.positive_int(period, 'period')

        super().__init__(params=[period, ma_type.name])
        self.period = period
        self._rsi_max = 1.0
        self._average_gain = MovingAverageFactory.create(self.period, ma_type)
        self._average_loss = MovingAverageFactory.create(self.period, ma_type)
        self._last_point = 0.0
        self.value = 0.0

    @cython.binding(True)
    cpdef void update(self, double point):
        """
        Update the indicator with the given point value.

        :param point: The point value.
        """
        # Check if first input
        if not self.has_inputs:
            self._last_point = point
            self._set_has_inputs()

        cdef double gain = point - self._last_point

        if gain > 0.0:
            self._average_gain.update(gain)
            self._average_loss.update(0.0)
        elif gain < 0.0:
            self._average_loss.update(-gain)
            self._average_gain.update(0.0)
        else:
            self._average_gain.update(0.0)
            self._average_loss.update(0.0)

        # Initialization logic
        if not self.initialized:
            if self._average_gain.initialized and self._average_loss.initialized:
                self._set_initialized()

        if self._average_loss.value == 0.0:
            self.value = self._rsi_max
            return

        cdef double rs = self._average_gain.value / self._average_loss.value

        self.value = self._rsi_max - (self._rsi_max / (1.0 + rs))
        self._last_point = point

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._average_gain.reset()
        self._average_loss.reset()
        self._last_point = 0.0
        self.value = 0.0
