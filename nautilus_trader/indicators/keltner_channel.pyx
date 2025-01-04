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
from nautilus_trader.indicators.average.ma_factory import MovingAverageType

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.atr cimport AverageTrueRange
from nautilus_trader.indicators.base.indicator cimport Indicator


cdef class KeltnerChannel(Indicator):
    """
    The Keltner channel is a volatility based envelope set above and below a
    central moving average. Traditionally the middle band is an EMA based on the
    typical price (high + low + close) / 3, the upper band is the middle band
    plus the ATR. The lower band is the middle band minus the ATR.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    k_multiplier : double
        The multiplier for the ATR (> 0).
    ma_type : MovingAverageType
        The moving average type for the middle band (cannot be None).
    ma_type_atr : MovingAverageType
        The moving average type for the internal ATR (cannot be None).
    use_previous : bool
        The boolean flag indicating whether previous price values should be used.
    atr_floor : double
        The ATR floor (minimum) output value for the indicator (>= 0).
    """

    def __init__(
        self,
        int period,
        double k_multiplier,
        ma_type not None: MovingAverageType=MovingAverageType.EXPONENTIAL,
        ma_type_atr not None: MovingAverageType=MovingAverageType.SIMPLE,
        bint use_previous=True,
        double atr_floor=0,
    ):
        Condition.positive_int(period, "period")
        Condition.positive(k_multiplier, "k_multiplier")
        Condition.not_negative(atr_floor, "atr_floor")

        params = [
            period,
            k_multiplier,
            ma_type.name,
            ma_type_atr.name,
            use_previous,
            atr_floor,
        ]
        super().__init__(params=params)

        self.period = period
        self.k_multiplier = k_multiplier
        self._ma = MovingAverageFactory.create(period, ma_type)
        self._atr = AverageTrueRange(period, ma_type_atr, use_previous, atr_floor)
        self.upper = 0
        self.middle = 0
        self.lower = 0

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
            bar.close.as_double()
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

        self._ma.update_raw(typical_price)
        self._atr.update_raw(high, low, close)

        self.upper = self._ma.value + (self._atr.value * self.k_multiplier)
        self.middle = self._ma.value
        self.lower = self._ma.value - (self._atr.value * self.k_multiplier)

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
        self._ma.reset()
        self._atr.reset()
        self.upper = 0
        self.middle = 0
        self.lower = 0
