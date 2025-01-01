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

from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.average.ma_factory import MovingAverageType

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.stats cimport fast_std_with_mean
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.objects cimport Price


cdef class BollingerBands(Indicator):
    """
    A Bollinger Band® is a technical analysis tool defined by a set of
    trend lines plotted two standard deviations (positively and negatively) away
    from a simple moving average (SMA) of an instruments price, which can be
    adjusted to user preferences.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    k : double
        The standard deviation multiple for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the indicator.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    ValueError
        If `k` is not positive (> 0).
    """

    def __init__(
        self,
        int period,
        double k,
        ma_type not None: MovingAverageType=MovingAverageType.SIMPLE,
    ):
        Condition.positive_int(period, "period")
        Condition.positive(k, "k")
        super().__init__(params=[period, k, ma_type.name])

        self.period = period
        self.k = k
        self._ma = MovingAverageFactory.create(period, ma_type)
        self._prices = deque(maxlen=period)

        self.upper = 0.0
        self.middle = 0.0
        self.lower = 0.0

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given tick.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        cdef double bid = Price.raw_to_f64_c(tick._mem.bid_price.raw)
        cdef double ask = Price.raw_to_f64_c(tick._mem.ask_price.raw)
        cdef double mid = (ask + bid) / 2.0
        self.update_raw(ask, bid, mid)

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given tick.

        Parameters
        ----------
        tick : TradeTick
            The tick for the update.

        """
        Condition.not_none(tick, "tick")

        cdef double price = Price.raw_to_f64_c(tick._mem.price.raw)
        self.update_raw(price, price, price)

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

    cpdef void update_raw(self, double high, double low, double close):
        """
        Update the indicator with the given prices.

        Parameters
        ----------
        high : double
            The high price for calculations.
        low : double
            The low price for calculations.
        close : double
            The closing price for calculations

        """
        # Add data to queues
        cdef double typical = (high + low + close) / 3.0

        self._prices.append(typical)
        self._ma.update_raw(typical)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._prices) >= self.period:
                self._set_initialized(True)

        # Calculate values
        cdef double std = fast_std_with_mean(
            values=np.asarray(self._prices, dtype=np.float64),
            mean=self._ma.value,
        )

        # Set values
        self.upper = self._ma.value + (self.k * std)
        self.middle = self._ma.value
        self.lower = self._ma.value - (self.k * std)

    cpdef void _reset(self):
        self._ma.reset()
        self._prices.clear()

        self.upper = 0.0
        self.middle = 0.0
        self.lower = 0.0
