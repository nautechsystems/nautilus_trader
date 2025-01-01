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


cdef class AverageTrueRange(Indicator):
    """
    An indicator which calculates the average true range across a rolling window.
    Different moving average types can be selected for the inner calculation.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    ma_type : MovingAverageType
        The moving average type for the indicator (cannot be None).
    use_previous : bool
        The boolean flag indicating whether previous price values should be used.
        (note: only applicable for `update()`. `update_mid()` will need to
        use previous price.
    value_floor : double
        The floor (minimum) output value for the indicator (>= 0).
    """

    def __init__(
        self,
        int period,
        ma_type not None: MovingAverageType=MovingAverageType.SIMPLE,
        bint use_previous=True,
        double value_floor=0,
    ):
        Condition.positive_int(period, "period")
        Condition.not_negative(value_floor, "value_floor")
        params = [
            period,
            ma_type.name,
            use_previous,
            value_floor,
        ]
        super().__init__(params=params)

        self.period = period
        self._ma = MovingAverageFactory.create(period, ma_type)
        self._use_previous = use_previous
        self._value_floor = value_floor
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

        self.update_raw(bar.high.as_double(), bar.low.as_double(), bar.close.as_double())

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
        # Calculate average
        if self._use_previous:
            if not self.has_inputs:
                self._previous_close = close
            self._ma.update_raw(max(self._previous_close, high) - min(low, self._previous_close))
            self._previous_close = close
        else:
            self._ma.update_raw(high - low)

        self._floor_value()
        self._check_initialized()

    cdef void _floor_value(self):
        if self._value_floor == 0:
            self.value = self._ma.value
        elif self._value_floor < self._ma.value:
            self.value = self._ma.value
        else:
            # Floor the value
            self.value = self._value_floor

    cdef void _check_initialized(self):
        if not self.initialized:
            self._set_has_inputs(True)
            if self._ma.initialized:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._ma.reset()
        self._previous_close = 0
        self.value = 0
