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

import numpy as np

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class AroonOscillator(Indicator):
    """
    The Aroon (AR) indicator developed by Tushar Chande attempts to
    determine whether an instrument is trending, and how strong the trend is.
    AroonUp and AroonDown lines make up the indicator with their formulas below.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    """

    def __init__(self, int period):
        Condition.positive_int(period, "period")
        params = [
            period,
        ]
        super().__init__(params = params)

        self.period = period
        self._high_inputs = deque(maxlen = self.period + 1)
        self._low_inputs = deque(maxlen = self.period + 1)
        self.aroon_up = 0
        self.aroon_down = 0
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

        self.update_raw(
            bar.high.as_double(),
            bar.low.as_double(),
        )

    cpdef void update_raw(
        self,
        double high,
        double low,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.
        """
        # Update inputs
        self._high_inputs.appendleft(high)
        self._low_inputs.appendleft(low)

        # Convert to double to compute values
        cdef double periods_from_hh = np.argmax(self._high_inputs)
        cdef double periods_from_ll = np.argmin(self._low_inputs)

        self.aroon_up = 100.0 * (1.0 - periods_from_hh / self.period)
        self.aroon_down = 100.0 * (1.0 - periods_from_ll / self.period)
        self.value = self.aroon_up - self.aroon_down

        self._check_initialized()

    cdef void _check_initialized(self):
        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._high_inputs) >= self.period + 1:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._high_inputs.clear()
        self._low_inputs.clear()
        self.aroon_up = 0
        self.aroon_down = 0
        self.value = 0
