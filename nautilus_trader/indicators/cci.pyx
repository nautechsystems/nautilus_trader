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
from nautilus_trader.core.stats cimport fast_mad_with_mean
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class CommodityChannelIndex(Indicator):
    """
    Commodity Channel Index is a momentum oscillator used to primarily identify
    overbought and oversold levels relative to a mean.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    scalar : double
        A positive float to scale the bands
    ma_type : MovingAverageType
        The moving average type for prices.

    References
    ----------
    https://www.tradingview.com/support/solutions/43000502001-commodity-channel-index-cci/
    """

    def __init__(
        self,
        int period,
        double scalar = 0.015,
        ma_type not None: MovingAverageType=MovingAverageType.SIMPLE,
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
        self._mad = 0.0
        self.value = 0.0

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
        cdef double typical_price = (high + low + close) / 3.0
        self._prices.append(typical_price)
        self._ma.update_raw(typical_price)
        self._mad = fast_mad_with_mean(
            values=np.asarray(self._prices, dtype=np.float64),
            mean=self._ma.value,
        )
        if self._ma.initialized:
            self.value = (typical_price - self._ma.value) / (self.scalar * self._mad)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self._ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        """
        Reset the indicator.

        All stateful fields are reset to their initial value.
        """
        self._prices.clear()
        self._ma.reset()
        self._mad = 0.0
        self.value = 0.0
