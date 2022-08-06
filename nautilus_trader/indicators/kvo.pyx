# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data.bar cimport Bar


cdef class KlingerVolumeOscillator(Indicator):
    """
    This indicator was developed by Stephen J. Klinger. It is designed to predict
    price reversals in a market by comparing volume to price.

    Parameters
    ----------
    fast_period : int
        The period for the fast moving average (> 0).
    slow_period : int
        The period for the slow moving average (> 0 & > fast_sma).
    signal_period : int
        The period for the moving average difference's moving average (> 0).
    ma_type : MovingAverageType
        The moving average type for the calculations.
    """

    def __init__(
        self,
        int fast_period,
        int slow_period,
        int signal_period,
        ma_type not None: MovingAverageType=MovingAverageType.EXPONENTIAL,
    ):
        Condition.positive_int(fast_period, "fast_period")
        Condition.positive_int(slow_period, "slow_period")
        Condition.true(slow_period > fast_period, "fast_period was >= slow_period")
        Condition.positive_int(signal_period, "signal_period")
        params = [
            fast_period,
            slow_period,
            signal_period,
            ma_type.name,
        ]
        super().__init__(params=params)

        self.fast_period = fast_period
        self.slow_period = slow_period
        self.signal_period = signal_period
        self._fast_ma = MovingAverageFactory.create(fast_period, ma_type)
        self._slow_ma = MovingAverageFactory.create(slow_period, ma_type)
        self._signal_ma = MovingAverageFactory.create(signal_period, ma_type)
        self._hlc3 = 0
        self._previous_hlc3 = 0
        self.value = 0

    cpdef void handle_bar(self, Bar bar) except *:
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
            bar.volume.as_double(),
        )

    cpdef void update_raw(
        self,
        double high,
        double low,
        double close,
        double volume,
    ) except *:
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
        volume : double
            The volume.

        """
        self._hlc3 = (high + low + close)/3.0

        if self._hlc3 > self._previous_hlc3:
            self._fast_ma.update_raw(volume)
            self._slow_ma.update_raw(volume)
        elif self._hlc3 < self._previous_hlc3:
            self._fast_ma.update_raw(-volume)
            self._slow_ma.update_raw(-volume)
        else:
            self._fast_ma.update_raw(0)
            self._slow_ma.update_raw(0)

        if self._slow_ma.initialized:
            self._signal_ma.update_raw(self._fast_ma.value - self._slow_ma.value)
            self.value = self._signal_ma.value

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self._signal_ma.initialized:
                self._set_initialized(True)

        self._previous_hlc3 = self._hlc3

    cpdef void _reset(self) except *:
        self._fast_ma.reset()
        self._slow_ma.reset()
        self._signal_ma.reset()
        self._hlc3 = 0
        self._previous_hlc3 = 0
        self.value = 0
