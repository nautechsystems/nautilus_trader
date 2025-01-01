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

import numpy as np

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.average.wma cimport WeightedMovingAverage
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.objects cimport Price


cdef class HullMovingAverage(MovingAverage):
    """
    An indicator which calculates a Hull Moving Average (HMA) across a rolling
    window. The HMA, developed by Alan Hull, is an extremely fast and smooth
    moving average.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    def __init__(self, int period, PriceType price_type=PriceType.LAST):
        Condition.positive_int(period, "period")
        super().__init__(period, params=[period], price_type=price_type)

        cdef int period_halved = int(self.period / 2)
        cdef int period_sqrt = int(np.sqrt(self.period))

        self._w1 = self._get_weights(period_halved)
        self._w2 = self._get_weights(period)
        self._w3 = self._get_weights(period_sqrt)

        self._ma1 = WeightedMovingAverage(period_halved, weights=self._w1)
        self._ma2 = WeightedMovingAverage(period, weights=self._w2)
        self._ma3 = WeightedMovingAverage(period_sqrt, weights=self._w3)

        self.value = 0

    cdef np.ndarray _get_weights(self, int size):
        cdef np.ndarray w = np.arange(1, size + 1, dtype=np.float64)
        w /= w.sum()
        return w

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        cdef Price price = tick.extract_price(self.price_type)
        self.update_raw(Price.raw_to_f64_c(price._mem.raw))

    cpdef void handle_trade_tick(self, TradeTick tick):
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        self.update_raw(Price.raw_to_f64_c(tick._mem.price.raw))

    cpdef void handle_bar(self, Bar bar):
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        self._ma1.update_raw(value)
        self._ma2.update_raw(value)
        self._ma3.update_raw(self._ma1.value * 2.0 - self._ma2.value)

        self.value = self._ma3.value
        self._increment_count()

    cpdef void _reset_ma(self):
        self._ma1.reset()
        self._ma2.reset()
        self._ma3.reset()
