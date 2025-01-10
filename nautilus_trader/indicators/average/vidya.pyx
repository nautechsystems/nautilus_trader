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

from nautilus_trader.indicators.average.ma_factory import MovingAverageType

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.cmo cimport ChandeMomentumOscillator
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.objects cimport Price


cdef class VariableIndexDynamicAverage(MovingAverage):
    """
    Variable Index Dynamic Average (VIDYA) was developed by Tushar Chande. It is
    similar to an Exponential Moving Average, but it has a dynamically adjusted
    lookback period dependent on relative price volatility as measured by Chande
    Momentum Oscillator (CMO). When volatility is high, VIDYA reacts faster to
    price changes. It is often used as moving average or trend identifier.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    price_type : PriceType
        The specified price type for extracting values from quotes.
    cmo_ma_type : int
        The moving average type for CMO indicator.

    Raises
    ------
    ValueError
        If `period` is not positive (> 0).
        If `cmo_ma_type` is ``VARIABLE_INDEX_DYNAMIC``.
    """

    def __init__(
        self,
        int period,
        PriceType price_type=PriceType.LAST,
        cmo_ma_type not None: MovingAverageType=MovingAverageType.SIMPLE,
    ):
        Condition.positive_int(period, "period")
        Condition.is_true(cmo_ma_type != MovingAverageType.VARIABLE_INDEX_DYNAMIC, "cmo_ma_type was invalid (VARIABLE_INDEX_DYNAMIC)")
        super().__init__(period, params=[period], price_type=price_type)

        self.cmo = ChandeMomentumOscillator(period, cmo_ma_type)
        self.cmo_pct = 0
        self.alpha = 2.0 / (period + 1.0)
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
        # Check if this is the initial input
        self.cmo.update_raw(value)
        self.cmo_pct = abs(self.cmo.value / 100)

        if self.initialized:
            self.value = self.alpha * self.cmo_pct * value + (1.0 - self.alpha *  self.cmo_pct) * self.value

        # Initialization logic
        if not self.initialized:
            if self.cmo.initialized:
                self._set_initialized(True)

        self._increment_count()
