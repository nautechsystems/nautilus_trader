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

from enum import Enum
from enum import unique

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.indicators.base.indicator cimport Indicator


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
    WILDER = 5
    DOUBLE_EXPONENTIAL = 6
    VARIABLE_INDEX_DYNAMIC = 7


cdef class MovingAverage(Indicator):
    """
    The base class for all moving average type indicators.

    Parameters
    ----------
    period : int
        The rolling window period for the indicator (> 0).
    params : list
        The initialization parameters for the indicator.
    price_type : PriceType, optional
        The specified price type for extracting values from quotes.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        int period,
        list params not None,
        PriceType price_type,
    ):
        Condition.positive_int(period, "period")
        super().__init__(params)

        self.period = period
        self.price_type = price_type
        self.value = 0
        self.count = 0

    cpdef void update_raw(self, double value):
        """
        Update the indicator with the given raw value.

        Parameters
        ----------
        value : double
            The update value.

        """
        raise NotImplementedError("method `update_raw` must be implemented in the subclass")  # pragma: no cover

    cpdef void _increment_count(self):
        self.count += 1

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            if self.count >= self.period:
                self._set_initialized(True)

    cpdef void _reset(self):
        self._reset_ma()
        self.count = 0
        self.value = 0

    cpdef void _reset_ma(self):
        pass  # Optionally override if additional values to reset
