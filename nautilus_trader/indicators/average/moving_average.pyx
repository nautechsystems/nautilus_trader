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

    def __init__(self,
                 int period,
                 list params not None,
                 PriceType price_type):
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

        self.period = period
        self.count = 0
        self.value = 0.0
        self._price_type = price_type

    cdef void _update(self) except *:
        """
        Update the indicator count and initialization properties.
        """
        self.count += 1

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self.count >= self.period:
                self._set_initialized(True)

    cdef void _reset_ma(self) except *:
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self.count = 0
        self.value = 0.0
