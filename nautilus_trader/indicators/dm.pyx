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
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class DirectionalMovement(Indicator):
    """
    Two oscillators that capture positive and negative trend movement.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the indicator (cannot be None).
    """

    def __init__(
        self,
        int period,
        ma_type not None: MovingAverageType=MovingAverageType.EXPONENTIAL,
    ):
        Condition.positive_int(period, "period")
        params = [
            period,
            ma_type.name,
        ]
        super().__init__(params=params)

        self.period = period
        self._pos_ma = MovingAverageFactory.create(period, ma_type)
        self._neg_ma = MovingAverageFactory.create(period, ma_type)
        self._previous_high = 0
        self._previous_low = 0
        self.pos = 0
        self.neg = 0

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
        )
    cpdef void update_raw(
        self,
        double high,
        double low,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        high : double
            The high price.
        low : double
            The low price.

        """
        if not self.has_inputs:
            self._previous_high = high
            self._previous_low = low

        cdef double up = high - self._previous_high
        cdef double dn = self._previous_low - low

        self._pos_ma.update_raw(((up > dn) and (up > 0)) * up)
        self._neg_ma.update_raw(((dn > up) and (dn > 0)) * dn)
        self.pos = self._pos_ma.value
        self.neg = self._neg_ma.value

        self._previous_high = high
        self._previous_low = low

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self._neg_ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        """
        Reset the indicator.

        All stateful fields are reset to their initial value.
        """
        self._pos_ma.reset()
        self._neg_ma.reset()
        self._previous_high = 0
        self._previous_low = 0
        self.pos = 0
        self.neg = 0
