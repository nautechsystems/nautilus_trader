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

from libc.math cimport fabs

from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.average.ma_factory import MovingAverageType

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class VerticalHorizontalFilter(Indicator):
    """
    The Vertical Horizon Filter (VHF) was created by Adam White to identify
    trending and ranging markets.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the indicator (cannot be None).
    """

    def __init__(
        self,
        int period,
        ma_type not None: MovingAverageType=MovingAverageType.SIMPLE,
    ):
        Condition.positive_int(period, "period")
        params = [
            period,
            ma_type.name,
        ]
        super().__init__(params=params)

        self.period = period
        self._prices = deque(maxlen=self.period)
        self._ma = MovingAverageFactory.create(period, ma_type)
        self._previous_close = 0
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
            bar.close.as_double(),
        )

    cpdef void update_raw(self, double close):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        close : double
            The close price.

        """
        # Update inputs
        if not self.has_inputs:
            self._previous_close = close

        self._prices.append(close)

        cdef double max_price = max(self._prices)
        cdef double min_price = min(self._prices)

        self._ma.update_raw(fabs(close - self._previous_close))
        if self.initialized:
            self.value = fabs(max_price - min_price) / self.period / self._ma.value
        self._previous_close = close

        self._check_initialized()

    cdef void _check_initialized(self):
        if not self.initialized:
            self._set_has_inputs(True)
            if self._ma.initialized and len(self._prices) >= self.period:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._prices.clear()
        self._ma.reset()
        self._previous_close = 0
        self.value = 0
