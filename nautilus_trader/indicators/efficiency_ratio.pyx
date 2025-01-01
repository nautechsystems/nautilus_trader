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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class EfficiencyRatio(Indicator):
    """
    An indicator which calculates the efficiency ratio across a rolling window.
    The Kaufman Efficiency measures the ratio of the relative market speed in
    relation to the volatility, this could be thought of as a proxy for noise.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (>= 2).

    Raises
    ------
    ValueError
        If `period` is not >= 2.
    """

    def __init__(self, int period):
        Condition.is_true(period >= 2, "period was < 2")
        super().__init__(params=[period])

        self.period = period
        self._inputs = deque(maxlen=period)
        self._deltas = deque(maxlen=period)
        self.value = 0

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

    cpdef void update_raw(self, double price):
        """
        Update the indicator with the given price.

        Parameters
        ----------
        price : double
            The update price.

        """
        self._inputs.append(price)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._inputs) < 2:
                return  # Not enough data
            elif len(self._inputs) >= self.period:
                self._set_initialized(True)

        # Add data to queues
        self._deltas.append(abs(self._inputs[-1] - self._inputs[-2]))

        # Calculate efficiency ratio
        cdef double net_diff = abs(self._inputs[0] - self._inputs[-1])
        cdef double sum_deltas = sum(self._deltas)

        if sum_deltas > 0:
            self.value = net_diff / sum_deltas
        else:
            self.value = 0

    cpdef void _reset(self):
        self._inputs.clear()
        self._deltas.clear()
        self.value = 0
