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
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.objects cimport Price


cdef class WeightedMovingAverage(MovingAverage):
    """
    An indicator which calculates a weighted moving average across a rolling window.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    weights : iterable
        The weights for the moving average calculation (if not ``None`` then = period).
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
    """

    def __init__(
        self,
        int period,
        weights = None,
        PriceType price_type = PriceType.LAST,
    ):
        Condition.positive_int(period, "period")
        if weights is not None:
            if not isinstance(weights, np.ndarray):
                # convert weights to the np.ndarray if it's possible
                weights = np.asarray(weights, dtype=np.float64)
                # to avoid the case when weights = [[1.0, 2.0, 3,0], [1.0, 2.0, 3,0]] ...
            if weights.ndim != 1:
                raise ValueError("weights must be iterable with ndim == 1.")
            else:
                Condition.is_true(weights.dtype == np.float64, "weights ndarray.dtype must be 'float64'")
                Condition.is_true(weights.ndim == 1, "weights ndarray.ndim must be 1")
            Condition.equal(len(weights), period, "len(weights)", "period")
            eps = np.finfo(np.float64).eps
            Condition.is_true(eps < weights.sum(), f"sum of weights must be positive > {eps}")
        super().__init__(period, params=[period, weights], price_type=price_type)

        self._inputs = deque(maxlen=period)
        self.weights = weights
        self.value = 0

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
        self._inputs.append(value)

        if self.initialized or self.weights is None:
            self.value = np.average(self._inputs, weights=self.weights, axis=0)
        else:
            self.value = np.average(self._inputs, weights=self.weights[-len(self._inputs):], axis=0)

        self._increment_count()

    cpdef void _reset_ma(self):
        self._inputs.clear()
