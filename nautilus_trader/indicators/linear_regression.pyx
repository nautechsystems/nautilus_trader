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

from collections import deque
from statistics import mean

import numpy as np

cimport numpy as np

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class LinearRegression(Indicator):
    """
    An indicator that calculates a simple linear regression.

    Parameters
    ----------
    period : int
        The period for the indicator.

    Raises
    ------
    ValueError
        If `period` is not greater than zero.
    """

    def __init__(self, int period=0):
        Condition.positive_int(period, "period")
        super().__init__(params=[period])

        self.period = period
        self._inputs = deque(maxlen=self.period)
        self.slope = 0.0
        self.intercept = 0.0
        self.degree = 0.0
        self.cfo = 0.0
        self.R2 = 0.0
        self.value = 0.0

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double close):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        close_price : double
            The close price.

        """
        self._inputs.append(close)

        # Warmup indicator logic
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._inputs) >= self.period:
                self._set_initialized(True)
            else:
                return

        cdef np.ndarray x_arr = np.arange(1, self.period + 1, dtype=np.float64)
        cdef np.ndarray y_arr = np.asarray(self._inputs, dtype=np.float64)
        cdef double x_sum = 0.5 * self.period * (self.period + 1)
        cdef double x2_sum = x_sum * (2 * self.period + 1) / 3
        cdef double divisor = self.period * x2_sum - x_sum * x_sum
        cdef double y_sum = sum(y_arr)
        cdef double xy_sum = sum(x_arr * y_arr)
        self.slope = (self.period * xy_sum - x_sum * y_sum) / divisor
        self.intercept = (y_sum * x2_sum - x_sum * xy_sum) / divisor

        cdef np.ndarray residuals = np.zeros(self.period, dtype=np.float64)
        cdef int i
        for i in np.arange(self.period):
            residuals[i] = self.slope * x_arr[i] + self.intercept - y_arr[i]

        self.value = residuals[-1] + y_arr[-1]
        self.degree = 180.0 / np.pi * np.arctan(self.slope)
        self.cfo = 100.0 * residuals[-1] / y_arr[-1]
        self.R2 = 1.0 - sum(residuals * residuals) / sum((y_arr - mean(y_arr)) * (y_arr - mean(y_arr)))

    cpdef void _reset(self):
        self._inputs.clear()
        self.slope = 0.0
        self.intercept = 0.0
        self.degree = 0.0
        self.cfo = 0.0
        self.R2 = 0.0
        self.value = 0.0
