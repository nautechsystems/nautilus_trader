# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
import talib

cimport numpy as np

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data.bar cimport Bar


cdef class Pattern(Indicator):
    """
    Candlestick patterns recognition from talib.
    Parameters
    ----------
    period : int
        The period for pattern recognition(> 0).
    pattern_names : list
        A list of pattern names provide for the indicator.
    """

    def __init__(
        self,
        int period = 30,
        list pattern_names = [
            "CDLDOJISTAR",
            "CDLDRAGONFLYDOJI",
            "CDLENGULFING",
            "CDLHAMMER",
            "CDLHARAMI",
            "CDLTAKURI",
            "CDLXSIDEGAP3METHODS",
            ],
    ):
        params=[
            period,
            pattern_names,
        ]
        super().__init__(params=params)
        self.period = period
        self._open_inputs = deque(maxlen=self.period)
        self._high_inputs = deque(maxlen=self.period)
        self._low_inputs = deque(maxlen=self.period)
        self._close_inputs = deque(maxlen=self.period)
        self.pattern_names = pattern_names
        self.value = [0 for item in self.pattern_names]

    cpdef void handle_bar(self, Bar bar) except *:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(
            bar.open.as_double(),
            bar.high.as_double(),
            bar.low.as_double(),
            bar.close.as_double(),
        )

    cpdef void update_raw(
        self,
        double open,
        double high,
        double low,
        double close,
    ) except *:
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        open : double
            The open price.
        high : double
            The high price.
        low : double
            The low price.
        close : double
            The close price.

        """
        self._open_inputs.append(open)
        self._high_inputs.append(high)
        self._low_inputs.append(low)
        self._close_inputs.append(close)

        cdef np.ndarray open_array =  np.asarray(self._open_inputs, dtype=np.float64)
        cdef np.ndarray high_array =  np.asarray(self._high_inputs, dtype=np.float64)
        cdef np.ndarray low_array =  np.asarray(self._low_inputs, dtype=np.float64)
        cdef np.ndarray close_array =  np.asarray(self._close_inputs, dtype=np.float64)

        cdef int i
        for i in range(len(self.pattern_names)):
            self.value[i] = getattr(talib, self.pattern_names[i])(
                open_array,
                high_array,
                low_array,
                close_array,
            )[-1]

        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._open_inputs) >= self.period:
                self._set_initialized(True)

    cpdef void _reset(self) except *:
        self._open_inputs.clear()
        self._high_inputs.clear()
        self._low_inputs.clear()
        self._close_inputs.clear()
