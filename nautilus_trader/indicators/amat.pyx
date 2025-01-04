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

from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.average.moving_average import MovingAverageType

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class ArcherMovingAveragesTrends(Indicator):
    """
    Archer Moving Averages Trends indicator.

    Parameters
    ----------
    fast_period : int
        The period for the fast moving average (> 0).
    slow_period : int
        The period for the slow moving average (> 0 & > fast_sma).
    signal_period : int
        The period for lookback price array (> 0).
    ma_type : MovingAverageType
        The moving average type for the calculations.

    References
    ----------
    https://github.com/twopirllc/pandas-ta/blob/bc3b292bf1cc1d5f2aba50bb750a75209d655b37/pandas_ta/trend/amat.py
    """

    def __init__(
        self,
        int fast_period,
        int slow_period,
        int signal_period,
        ma_type not None: MovingAverageType=MovingAverageType.EXPONENTIAL,
    ):
        Condition.positive_int(fast_period, "fast_period")
        Condition.positive_int(slow_period, "slow_period")
        Condition.is_true(slow_period > fast_period, "fast_period was >= slow_period")
        Condition.positive_int(signal_period, "signal_period")
        params = [
            fast_period,
            slow_period,
            signal_period,
            ma_type.name,
        ]
        super().__init__(params=params)

        self.fast_period = fast_period
        self.slow_period = slow_period
        self.signal_period = signal_period
        self._fast_ma = MovingAverageFactory.create(fast_period, ma_type)
        self._slow_ma = MovingAverageFactory.create(slow_period, ma_type)
        self._fast_ma_price = deque(maxlen = signal_period + 1)
        self._slow_ma_price = deque(maxlen = signal_period + 1)
        self.long_run = 0
        self.short_run = 0

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
        Update the indicator with the given close price value.

        Parameters
        ----------
        close : double
            The close price.

        """
        self._fast_ma.update_raw(close)
        self._slow_ma.update_raw(close)
        if self._slow_ma.initialized:
            self._fast_ma_price.append(self._fast_ma.value)
            self._slow_ma_price.append(self._slow_ma.value)

            self.long_run = (self._fast_ma_price[-1] - self._fast_ma_price[0] > 0 and \
                self._slow_ma_price[-1] - self._slow_ma_price[0] < 0 )
            self.long_run = (self._fast_ma_price[-1] - self._fast_ma_price[0] > 0 and \
                self._slow_ma_price[-1] - self._slow_ma_price[0] > 0 ) or self.long_run

            self.short_run = (self._fast_ma_price[-1] - self._fast_ma_price[0] < 0 and \
                self._slow_ma_price[-1] - self._slow_ma_price[0] > 0 )
            self.short_run = (self._fast_ma_price[-1] - self._fast_ma_price[0] < 0 and \
                self._slow_ma_price[-1] - self._slow_ma_price[0] < 0 ) or self.short_run

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._slow_ma_price) >= self.signal_period + 1 and self._slow_ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._fast_ma.reset()
        self._slow_ma.reset()
        self._fast_ma_price.clear()
        self._slow_ma_price.clear()
        self.long_run = 0
        self.short_run = 0
