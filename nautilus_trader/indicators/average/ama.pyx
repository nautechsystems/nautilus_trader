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

from libc.math cimport pow

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.efficiency_ratio cimport EfficiencyRatio
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.objects cimport Price


cdef class AdaptiveMovingAverage(MovingAverage):
    """
    An indicator which calculates an adaptive moving average (AMA) across a
    rolling window. Developed by Perry Kaufman, the AMA is a moving average
    designed to account for market noise and volatility. The AMA will closely
    follow prices when the price swings are relatively small and the noise is
    low. The AMA will increase lag when the price swings increase.

    Parameters
    ----------
    period_er : int
        The period for the internal `EfficiencyRatio` indicator (> 0).
    period_alpha_fast : int
        The period for the fast smoothing constant (> 0).
    period_alpha_slow : int
        The period for the slow smoothing constant (> 0 < alpha_fast).
    price_type : PriceType
        The specified price type for extracting values from quotes.
    """

    def __init__(
        self,
        int period_er,
        int period_alpha_fast,
        int period_alpha_slow,
        PriceType price_type=PriceType.LAST,
    ):
        Condition.positive_int(period_er, "period_er")
        Condition.positive_int(period_alpha_fast, "period_alpha_fast")
        Condition.positive_int(period_alpha_slow, "period_alpha_slow")
        Condition.is_true(period_alpha_slow > period_alpha_fast, "period_alpha_slow was <= period_alpha_fast")

        params = [
            period_er,
            period_alpha_fast,
            period_alpha_slow
        ]
        super().__init__(period_er, params=params, price_type=price_type)

        self.period_er = period_er
        self.period_alpha_fast = period_alpha_fast
        self.period_alpha_slow = period_alpha_slow
        self.alpha_fast = 2.0 / (float(period_alpha_fast) + 1.0)
        self.alpha_slow = 2.0 / (float(period_alpha_slow) + 1.0)
        self.alpha_diff = self.alpha_fast - self.alpha_slow
        self._efficiency_ratio = EfficiencyRatio(self.period_er)
        self._prior_value = 0
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
        # Check if this is the initial input (then initialize variables)
        if not self.has_inputs:
            self.value = value

        self._efficiency_ratio.update_raw(value)
        self._prior_value = self.value

        # Calculate smoothing constant (sc)
        cdef double sc = pow(self._efficiency_ratio.value * self.alpha_diff + self.alpha_slow, 2)

        # Calculate AMA
        self.value = self._prior_value + sc * (value - self._prior_value)

        self._increment_count()

    cpdef void _reset_ma(self):
        self._efficiency_ratio.reset()
        self._prior_value = 0
