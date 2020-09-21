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

import numpy as np

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef class WeightedMovingAverage(MovingAverage):
    """
    An indicator which calculates a weighted moving average across a rolling window.
    """

    def __init__(self,
                 int period,
                 weights=None,
                 PriceType price_type=PriceType.UNDEFINED):
        """
        Initialize a new instance of the SimpleMovingAverage class.

        :param period: The rolling window period for the indicator (> 0).
        :param weights: The weights for the moving average calculation
        (if not None then = period).
        :param price_type: The specified price type for extracting values from quote ticks (default=UNDEFINED).
        """
        Condition.positive_int(period, "period")
        if weights is not None:
            Condition.equal(len(weights), period, "len(weights)", "period")
        super().__init__(period, params=[period, weights], price_type=price_type)

        self._inputs = deque(maxlen=self.period)
        self.weights = weights
        self.value = 0.0

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        """
        Update the indicator with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        self.update_raw(tick.extract_price(self._price_type).as_double())

    cpdef void handle_trade_tick(self, TradeTick tick) except *:
        """
        Update the indicator with the given trade tick.

        Parameters
        ----------
        tick : TradeTick
            The update tick to handle.

        """
        Condition.not_none(tick, "tick")

        self.update_raw(tick.price.as_double())

    cpdef void handle_bar(self, Bar bar) except *:
        """
        Update the indicator with the given bar.

        Parameters
        ----------
        bar : Bar
            The update bar to handle.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double value) except *:
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        self._update()
        self._inputs.append(value)

        if self.initialized or self.weights is None:
            self.value = np.average(self._inputs, weights=self.weights, axis=0)
        else:
            self.value = np.average(self._inputs, weights=self.weights[-len(self._inputs):], axis=0)

    cpdef void reset(self) except *:
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_ma()
        self._inputs.clear()
