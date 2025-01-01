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


cdef class OnBalanceVolume(Indicator):
    """
    An indicator which calculates the momentum of relative positive or negative
    volume.

    Parameters
    ----------
    period : int
        The period for the indicator, zero indicates no window (>= 0).

    Raises
    ------
    ValueError
        If `period` is negative (< 0).
    """

    def __init__(self, int period=0):
        Condition.not_negative(period, "period")
        super().__init__(params=[period])

        self.period = period
        self._obv = deque(maxlen=None if period == 0 else period)
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
            bar.open.as_double(),
            bar.close.as_double(),
            bar.volume.as_double(),
        )

    cpdef void update_raw(
        self,
        double open,
        double close,
        double volume,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        open : double
            The high price.
        close : double
            The low price.
        volume : double
            The close price.

        """
        if close > open:
            self._obv.append(volume)
        elif close < open:
            self._obv.append(-volume)
        else:
            self._obv.append(0)

        self.value = sum(self._obv)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if (self.period == 0 and len(self._obv) > 0) or len(self._obv) >= self.period:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._obv.clear()
        self.value = 0
