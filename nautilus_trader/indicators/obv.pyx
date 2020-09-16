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

from collections import deque

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.bar cimport Bar


cdef class OnBalanceVolume(Indicator):
    """
    An indicator which calculates the momentum of relative positive or negative volume.
    """

    def __init__(self, int period=0):
        """
        Initialize a new instance of the OnBalanceVolume class.

        :param period: The period for the indicator, zero indicates no window (>= 0).
        """
        Condition.not_negative(period, "period")
        super().__init__(params=[period])

        self.period = period
        self._obv = deque(maxlen=None if self.period == 0 else self.period)
        self.value = 0.0

    cpdef void update(self, Bar bar) except *:
        """
        Update the indicator with the given bar.

        :param bar: The update bar.
        """
        Condition.not_none(bar, "bar")

        self.update_raw(
            bar.open.as_double(),
            bar.close.as_double(),
            bar.volume.as_double()
        )

    cpdef void update_raw(
            self,
            double open_price,
            double close_price,
            double volume) except *:
        """
        Update the indicator with the given raw values.

        :param open_price: The high price.
        :param close_price: The low price.
        :param volume: The close price.
        """
        if close_price > open_price:
            self._obv.append(volume)
        elif close_price < open_price:
            self._obv.append(-volume)
        else:
            self._obv.append(0)

        self.value = sum(self._obv)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if (self.period == 0 and len(self._obv) > 0) or len(self._obv) >= self.period:
                self._set_initialized(True)

    cpdef void reset(self) except *:
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._obv.clear()
