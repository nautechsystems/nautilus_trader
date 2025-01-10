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

from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.average.moving_average import MovingAverageType

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.objects cimport Price


cdef class MovingAverageConvergenceDivergence(Indicator):
    """
    An indicator which calculates the difference between two moving averages.
    Different moving average types can be selected for the inner calculation.

    Parameters
    ----------
    fast_period : int
        The period for the fast moving average (> 0).
    slow_period : int
        The period for the slow moving average (> 0 & > fast_sma).
    ma_type : MovingAverageType
        The moving average type for the calculations.
    price_type : PriceType
        The specified price type for extracting values from quotes.

    Raises
    ------
    ValueError
        If `fast_period` is not positive (> 0).
    ValueError
        If `slow_period` is not positive (> 0).
    ValueError
        If `fast_period` is not < `slow_period`.
    """

    def __init__(
        self,
        int fast_period,
        int slow_period,
        ma_type not None: MovingAverageType=MovingAverageType.EXPONENTIAL,
        PriceType price_type=PriceType.LAST,
    ):
        Condition.positive_int(fast_period, "fast_period")
        Condition.positive_int(slow_period, "slow_period")
        Condition.is_true(slow_period > fast_period, "slow_period was <= fast_period")

        params=[
            fast_period,
            slow_period,
            ma_type.name,
        ]
        super().__init__(params=params)

        self.fast_period = fast_period
        self.slow_period = slow_period
        self._fast_ma = MovingAverageFactory.create(fast_period, ma_type)
        self._slow_ma = MovingAverageFactory.create(slow_period, ma_type)
        self.price_type = price_type
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
            The update bar.

        """
        Condition.not_none(bar, "bar")

        self.update_raw(bar.close.as_double())

    cpdef void update_raw(self, double close):
        """
        Update the indicator with the given close price.

        Parameters
        ----------
        close : double
            The close price.

        """
        self._fast_ma.update_raw(close)
        self._slow_ma.update_raw(close)
        self.value = self._fast_ma.value - self._slow_ma.value

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self._fast_ma.initialized and self._slow_ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._fast_ma.reset()
        self._slow_ma.reset()
        self.value = 0
