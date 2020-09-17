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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator

from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.average.ma_factory import MovingAverageType

from nautilus_trader.indicators.atr cimport AverageTrueRange


cdef class KeltnerChannel(Indicator):
    """
    An indicator which provides a Keltner channel. The Keltner channel is a
    volatility based envelope set above and below a central moving average.
    Traditionally the middle band is an EMA based on the typical price
    ((high + low + close) / 3), the upper band is the middle band plus the ATR.
    The lower band is the middle band minus the ATR.
    """

    def __init__(self,
                 int period,
                 double k_multiplier,
                 ma_type not None: MovingAverageType=MovingAverageType.EXPONENTIAL,
                 ma_type_atr not None: MovingAverageType=MovingAverageType.SIMPLE,
                 bint use_previous=True,
                 double atr_floor=0.0):
        """
        Initialize a new instance of the KeltnerChannel class.

        :param period: The rolling window period for the indicator (> 0).
        :param k_multiplier: The multiplier for the ATR (> 0).
        :param ma_type: The moving average type for the middle band (cannot be None).
        :param ma_type_atr: The moving average type for the internal ATR (cannot be None).
        :param use_previous: The boolean flag indicating whether previous price values should be used.
        :param atr_floor: The ATR floor (minimum) output value for the indicator (>= 0).
        """
        Condition.positive_int(period, "period")
        Condition.positive(k_multiplier, "k_multiplier")
        Condition.not_negative(atr_floor, "atr_floor")
        super().__init__(params=[period,
                                 k_multiplier,
                                 ma_type.name,
                                 ma_type_atr.name,
                                 use_previous,
                                 atr_floor])
        self.period = period
        self.k_multiplier = k_multiplier
        self._moving_average = MovingAverageFactory.create(self.period, ma_type)
        self._atr = AverageTrueRange(self.period, ma_type_atr, use_previous, atr_floor)
        self.value_upper_band = 0.0
        self.value_middle_band = 0.0
        self.value_lower_band = 0.0

    cpdef void handle_bar(self, Bar bar) except *:
        """
        Update the indicator with the given bar.

        :param bar: The update bar.
        """
        Condition.not_none(bar, "bar")

        self.update_raw(
            bar.high.as_double(),
            bar.low.as_double(),
            bar.close.as_double()
        )

    cpdef void update_raw(self, double high, double low, double close) except *:
        """
        Update the indicator with the given raw value.

        :param high: The high price.
        :param low: The low price.
        :param close: The close price.
        """
        cdef double typical_price = (high + low + close) / 3.0

        self._moving_average.update_raw(typical_price)
        self._atr.update_raw(high, low, close)

        self.value_upper_band = self._moving_average.value + (self._atr.value * self.k_multiplier)
        self.value_middle_band = self._moving_average.value
        self.value_lower_band = self._moving_average.value - (self._atr.value * self.k_multiplier)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self._moving_average.initialized:
                self._set_initialized(True)

    cpdef void reset(self) except *:
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._moving_average.reset()
        self._atr.reset()
        self.value_upper_band = 0.0
        self.value_middle_band = 0.0
        self.value_lower_band = 0.0
