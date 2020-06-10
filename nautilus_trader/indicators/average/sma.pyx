# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import cython
from collections import deque

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport fast_mean
from nautilus_trader.indicators.average.moving_average cimport MovingAverage


cdef class SimpleMovingAverage(MovingAverage):
    """
    An indicator which calculates a simple moving average across a rolling window.
    """

    def __init__(self, int period):
        """
        Initializes a new instance of the SimpleMovingAverage class.

        :param period: The rolling window period for the indicator (> 0).
        """
        Condition.positive_int(period, 'period')

        super().__init__(period, params=[period])
        self._inputs = deque(maxlen=period)
        self.value = 0.0

    @cython.binding(True)
    cpdef update(self, double point):
        """
        Update the indicator with the given point value.

        :param point: The input point value for the update.
        """
        self._update(point)
        self._inputs.append(point)

        self.value = fast_mean(list(self._inputs))

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_ma()
        self._inputs.clear()
