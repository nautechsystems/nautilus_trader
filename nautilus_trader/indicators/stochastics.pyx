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


cdef class Stochastics(Indicator):
    """
    An oscillator which can indicate when an asset may be over bought or over
    sold.

    Parameters
    ----------
    period_k : int
        The period for the K line.
    period_d : int
        The period for the D line.

    Raises
    ------
    ValueError
        If `period_k` is not positive (> 0).
    ValueError
        If `period_d` is not positive (> 0).

    References
    ----------
    https://www.forextraders.com/forex-education/forex-indicators/stochastics-indicator-explained/
    """

    def __init__(self, int period_k, int period_d):
        Condition.positive_int(period_k, "period_k")
        Condition.positive_int(period_d, "period_d")
        super().__init__(params=[period_k, period_d])

        self.period_k = period_k
        self.period_d = period_d
        self._highs = deque(maxlen=period_k)
        self._lows = deque(maxlen=period_k)
        self._c_sub_l = deque(maxlen=period_d)
        self._h_sub_l = deque(maxlen=period_d)

        self.value_k = 0
        self.value_d = 0

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
            bar.close.as_double(),
        )

    cpdef void update_raw(
        self,
        double high,
        double low,
        double close,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.
        close : double
            The close price.

        """
        # Check if first input
        if not self.has_inputs:
            self._set_has_inputs(True)

        self._highs.append(high)
        self._lows.append(low)

        # Initialization logic
        if not self.initialized:
            if len(self._highs) == self.period_k and len(self._lows) == self.period_k:
                self._set_initialized(True)

        cdef double k_max_high = max(self._highs)
        cdef double k_min_low = min(self._lows)

        self._c_sub_l.append(close - k_min_low)
        self._h_sub_l.append(k_max_high - k_min_low)

        if k_max_high == k_min_low:
            return  # Divide by zero guard

        self.value_k = 100 * ((close - k_min_low) / (k_max_high - k_min_low))
        self.value_d = 100 * (sum(self._c_sub_l) / sum(self._h_sub_l))

    cpdef void _reset(self):
        self._highs.clear()
        self._lows.clear()
        self._c_sub_l.clear()
        self._h_sub_l.clear()

        self.value_k = 0
        self.value_d = 0
