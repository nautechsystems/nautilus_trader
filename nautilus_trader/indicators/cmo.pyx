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
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class ChandeMomentumOscillator(Indicator):
    """
    Attempts to capture the momentum of an asset with overbought at 50 and
    oversold at -50.

    Parameters
    ----------
    ma_type : int
        The moving average type for average gain/loss.
    period : MovingAverageType
        The rolling window period for the indicator.
    """

    def __init__(
        self,
        int period,
        ma_type not None: MovingAverageType=MovingAverageType.WILDER,
    ):
        params = [
            period,
            ma_type.name,
        ]
        super().__init__(params = params)

        self.period = period
        self._average_gain = MovingAverageFactory.create(period, ma_type)
        self._average_loss = MovingAverageFactory.create(period, ma_type)
        self._previous_close = 0
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
        Update the indicator with the given value.

        Parameters
        ----------
        value : double
            The update value.

        """
        # Check if first input
        if not self.has_inputs:
            self._set_has_inputs(True)
            self._previous_close = close

        cdef double gain = close - self._previous_close

        if gain > 0:
            self._average_gain.update_raw(gain)
            self._average_loss.update_raw(0)
        elif gain < 0:
            self._average_gain.update_raw(0)
            self._average_loss.update_raw(-gain)
        else:
            self._average_gain.update_raw(0)
            self._average_loss.update_raw(0)
        # Initialization logic
        if not self.initialized:
            if self._average_gain.initialized and self._average_loss.initialized:
                self._set_initialized(True)

        cdef double divisor
        if self.initialized:
            divisor = self._average_gain.value + self._average_loss.value
            if divisor == 0.0:
                self.value = 0.0
            else:
                self.value = 100.0 * (self._average_gain.value - self._average_loss.value) / divisor

        self._previous_close = close

    cpdef void _reset(self):
        self._average_gain.reset()
        self._average_loss.reset()
        self._previous_close = 0
        self.value = 0
