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


from nautilus_trader.indicators.average.moving_average import MovingAverageType

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.indicators.keltner_channel cimport KeltnerChannel
from nautilus_trader.model.bar cimport Bar


cdef class KeltnerPosition(Indicator):
    """
    An indicator which calculates the relative position of the given price
    within a defined Keltner channel. This provides a measure of the relative
    'extension' of a market from the mean, as a multiple of volatility.
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
        self._period = period
        self._kc = KeltnerChannel(
            period,
            k_multiplier,
            ma_type,
            ma_type_atr,
            use_previous,
            atr_floor)
        self.value = 0.0

    @property
    def period(self) -> int:
        """
        :return: The period of the indicator.
        """
        return self._period

    @property
    def k_multiplier(self) -> float:
        """
        :return: The k-multiplier which calculates the upper and lower bands.
        """
        return self._kc.k_multiplier

    cpdef void update(self, Bar bar) except *:
        """
        Update the indicator with the given bar.

        :param bar: The update bar.
        """
        Condition.not_none(bar, "bar")

        self.update_raw(
            bar.high.as_double(),
            bar.low.as_double(),
            bar.close.as_double(),
        )

    cpdef void update_raw(self, double high, double low, double close) except *:
        """
        Update the indicator with the given raw value.

        :param high: The high price.
        :param low: The low price.
        :param close: The close price.
        """
        self._kc.update_raw(high, low, close)

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self._kc.initialized:
                self._set_initialized(True)

        cdef double k_width = (self._kc.value_upper_band - self._kc.value_lower_band) / 2

        if k_width > 0.0:
            self.value = (close - self._kc.value_middle_band) / k_width
        else:
            self.value = 0.0

    cpdef void reset(self) except *:
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._kc.reset()
        self.value = 0.0
