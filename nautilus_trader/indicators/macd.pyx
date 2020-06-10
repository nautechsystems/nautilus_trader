# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
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


cdef class MovingAverageConvergenceDivergence(Indicator):
    """
    An indicator which calculates the difference between two moving averages.
    Different moving average types can be selected for the inner calculation.
    """

    def __init__(self,
                 int fast_period,
                 int slow_period,
                 ma_type not None: MovingAverageType=MovingAverageType.EXPONENTIAL,
                 bint check_inputs=False):
        """
        Initializes a new instance of the MovingAverageConvergenceDivergence class.

        :param fast_period: The period for the fast moving average (> 0).
        :param slow_period: The period for the slow moving average (> 0 & > fast_sma).
        :param ma_type: The moving average type for the calculations.
        :param check_inputs: The flag indicating whether the input values should be checked.
        """
        Condition.positive_int(fast_period, 'fast_period')
        Condition.positive_int(slow_period, 'slow_period')
        Condition.true(slow_period > fast_period, 'slow_period > fast_period')

        super().__init__(params=[fast_period,
                                 slow_period,
                                 ma_type.name],
                         check_inputs=check_inputs)

        self._fast_period = fast_period
        self._slow_period = slow_period
        self._fast_ma = MovingAverageFactory.create(fast_period, ma_type)
        self._slow_ma = MovingAverageFactory.create(slow_period, ma_type)
        self.value = 0.0

    @cython.binding(True)
    cpdef void update(self, double point):
        """
        Update the indicator with the given point value.

        :param point: The price value.
        """
        self._fast_ma.update(point)
        self._slow_ma.update(point)
        self.value = self._fast_ma.value - self._slow_ma.value

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs()
            if self._fast_ma.initialized and self._slow_ma.initialized:
                self._set_initialized()

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._fast_ma.reset()
        self._slow_ma.reset()
        self.value = 0.0
