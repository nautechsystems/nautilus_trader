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

from enum import Enum
from enum import unique

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.c_enums.price_type cimport PriceType


@unique
class MovingAverageType(Enum):
    """
    Represents the type of moving average.
    """
    SIMPLE = 0
    EXPONENTIAL = 1
    WEIGHTED = 2
    HULL = 3
    ADAPTIVE = 4


cdef class MovingAverage(Indicator):
    """
    The base class for all moving average type indicators.
    """

    def __init__(
            self,
            int period,
            list params not None,
            PriceType price_type,
    ):
        """
        Initialize a new instance of the abstract MovingAverage class.

        Parameters
        ----------
        period : int
            The rolling window period for the indicator (> 0).
        params : list
            The initialization parameters for the indicator.
        price_type : PriceType, optional
            The specified price type for extracting values from quote ticks.

        """
        Condition.positive_int(period, "period")
        super().__init__(params)

        self._period = period
        self._price_type = price_type
        self._count = 0
        self._value = 0

    @property
    def period(self):
        """
        The indicators moving average period.

        Returns
        -------
        int

        """
        return self._period

    @property
    def price_type(self):
        """
        The specified price type for extracting values from quote ticks.

        Returns
        -------
        PriceType

        """
        return self._price_type

    @property
    def count(self):
        """
        The count of inputs received by the indicator.

        Returns
        -------
        int

        """
        return self._count

    @property
    def value(self):
        """
        The current moving average value.

        Returns
        -------
        double

        """
        return self._value

    cdef void _increment_count(self) except *:
        self._count += 1

        # Initialization logic
        if not self._initialized:
            self._set_has_inputs(True)
            if self._count >= self.period:
                self._set_initialized(True)

    cdef void _reset_ma(self) except *:
        """
        Reset the indicator.

        All stateful values are reset to their initial value.

        """
        self._reset_base()
        self._count = 0
        self._value = 0
