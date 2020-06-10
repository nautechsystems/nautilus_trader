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

from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.core.correctness cimport Condition


cdef class ExponentialMovingAverage(MovingAverage):
    """
    An indicator which calculates an exponential moving average across a
    rolling window.
    """

    def __init__(self, int period):
        """
        Initializes a new instance of the ExponentialMovingAverage class.

        :param period: The rolling window period for the indicator (> 0).
        """
        Condition.positive_int(period, 'period')

        super().__init__(period, params=[period])
        self.alpha = 2.0 / (period + 1.0)
        self.value = 0.0

    @cython.binding(True)
    cpdef update(self, double point):
        """
        Update the indicator with the given point value.

        :param point: The input point value for the update.
        """
        # Check if this is the initial input
        if not self.has_inputs:
            self._update(point)
            self.value = point
            return

        self._update(point)
        self.value = self.alpha * point + ((1.0 - self.alpha) * self.value)

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_ma()
