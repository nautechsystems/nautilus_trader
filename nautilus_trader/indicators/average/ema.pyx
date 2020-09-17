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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.model.bar cimport Bar


cdef class ExponentialMovingAverage(MovingAverage):
    """
    An indicator which calculates an exponential moving average across a
    rolling window.
    """

    def __init__(self, int period):
        """
        Initialize a new instance of the ExponentialMovingAverage class.

        :param period: The rolling window period for the indicator (> 0).
        """
        Condition.positive_int(period, "period")
        super().__init__(period, params=[period])

        self.alpha = 2.0 / (period + 1.0)
        self.value = 0.0

    cpdef void handle_bar(self, Bar bar) except *:
        """
        Update the indicator with the given bar.

        :param bar: The update bar.
        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value) except *:
        """
        Update the indicator with the given raw value.

        :param value: The update value.
        """
        # Check if this is the initial input
        if not self.has_inputs:
            self._update()
            self.value = value
            return

        self._update()
        self.value = self.alpha * value + ((1.0 - self.alpha) * self.value)

    cpdef void reset(self) except *:
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_ma()
