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
import numpy as np
from collections import deque

from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.core.correctness cimport Condition


cdef class WeightedMovingAverage(MovingAverage):
    """
    An indicator which calculates a weighted moving average across a rolling window.
    """

    def __init__(self,
                 int period,
                 weights=None):
        """
        Initializes a new instance of the SimpleMovingAverage class.

        :param period: The rolling window period for the indicator (> 0).
        :param weights: The weights for the moving average calculation
        (if not None then = period).
        """
        Condition.positive_int(period, 'period')
        if weights is not None:
            Condition.equal(len(weights), period, 'len(weights)', 'period')

        super().__init__(period, params=[period, weights])
        self._inputs = deque(maxlen=self.period)
        self.weights = weights
        self.value = 0.0

    @cython.binding(True)
    cpdef void update(self, double point):
        """
        Update the indicator with the given point value.

        :param point: The input point value for the update.
        """
        self._update(point)
        self._inputs.append(point)

        if self.initialized or self.weights is None:
            self.value = np.average(self._inputs, weights=self.weights, axis=0)
        else:
            self.value = np.average(self._inputs, weights=self.weights[-len(self._inputs):], axis=0)

    cpdef void reset(self):
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_ma()
        self._inputs.clear()
