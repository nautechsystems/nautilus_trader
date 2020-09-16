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

from collections import deque
from math import log

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.bar cimport Bar


cdef class RateOfChange(Indicator):
    """
    An indicator which calculates the rate of change of price over a defined period.
    The return output can be simple or log.
    """

    def __init__(self, int period, bint use_log=False):
        """
        Initialize a new instance of the RateOfChange class.

        :param period: The period for the indicator (> 1).
        :param use_log: Use log returns for value calculation.
        """
        Condition.true(period > 1, "period > 1")

        super().__init__(params=[period])
        self.period = period
        self._use_log = use_log
        self._prices = deque(maxlen=self.period)
        self.value = 0.0

    cpdef void update(self, Bar bar) except *:
        """
        Update the indicator with the given bar.

        :param bar: The update bar.
        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double price) except *:
        """
        Update the indicator with the given price.

        :param price: The update price.
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

    cpdef void reset(self) except *:
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._prices.clear()
        self.value = 0.0
