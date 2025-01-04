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


cdef class RelativeVolatilityIndex(Indicator):
    """
    The Relative Volatility Index (RVI) was created in 1993 and revised in 1995.
    Instead of adding up price changes like RSI based on price direction, the RVI
    adds up standard deviations based on price direction.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    scalar : double
        A positive float to scale the bands.
    ma_type : MovingAverageType
        The moving average type for the vip and vim (cannot be None).
    """

    def __init__(
        self,
        int period,
        double scalar = 100.0,
        ma_type not None: MovingAverageType=MovingAverageType.EXPONENTIAL,
    ):
        Condition.positive_int(period, "period")

        params = [
            period,
            scalar,
            ma_type.name,
        ]
        super().__init__(params=params)

        self.period = period
        self.scalar = scalar
        self._prices = deque(maxlen=period)
        self._ma = MovingAverageFactory.create(period, MovingAverageType.SIMPLE)
        self._pos_ma = MovingAverageFactory.create(period, ma_type)
        self._neg_ma = MovingAverageFactory.create(period, ma_type)
        self._previous_close = 0
        self._std = 0
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

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double close):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        close : double
            The close price.

        """
        self._prices.append(close)
        self._ma.update_raw(close)

        self._std = fast_std_with_mean(
            values=np.asarray(self._prices, dtype=np.float64),
            mean=self._ma.value,
        )

        self._std = self._std * np.sqrt(self.period) / np.sqrt(self.period - 1)

        if self._ma.initialized:
            if close > self._previous_close:
                self._pos_ma.update_raw(self._std)
                self._neg_ma.update_raw(0)
            elif close < self._previous_close:
                self._pos_ma.update_raw(0)
                self._neg_ma.update_raw(self._std)
            else:
                self._pos_ma.update_raw(0)
                self._neg_ma.update_raw(0)

            self.value = self.scalar * self._pos_ma.value
            self.value = self.value / (self._pos_ma.value + self._neg_ma.value)


        self._previous_close = close

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if  self._pos_ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        """
        Reset the indicator.

        All stateful fields are reset to their initial value.
        """
        self._prices.clear()
        self._ma.reset()
        self._pos_ma.reset()
        self._neg_ma.reset()
        self._previous_close = 0
        self._std = 0
        self.value = 0
