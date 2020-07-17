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

import cython
from cpython.datetime cimport datetime

from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.core.correctness cimport Condition


cdef class VolumeWeightedAveragePrice(Indicator):
    """
    An indicator which calculates the volume weighted average price for the day.
    """

    def __init__(self, bint check_inputs=False):
        """
        Initializes a new instance of the VolumeWeightedAveragePrice class.

        :param check_inputs: The flag indicating whether the input values should be checked.
        """
        super().__init__(params=[], check_inputs=check_inputs)

        self._day = 0
        self._price_volume = 0.0
        self._volume_total = 0.0
        self.value = 0.0

        self.has_inputs = False
        self.initialized = False

    @cython.binding(True)  # Needed for IndicatorUpdater to use this method as a delegate
    cpdef void update(
            self,
            double price,
            double volume,
            datetime timestamp) except *:
        """
        Update the indicator with the given values.

        :param price: The price (> 0).
        :param volume: The volume (>= 0).
        :param timestamp: The timestamp.
        """
        if self.check_inputs:
            Condition.positive(price, 'price')
            Condition.not_negative(volume, 'volume')

        # On a new day reset the indicator
        if timestamp.day != self._day:
            self.reset()
            self._day = timestamp.day
            self.value = price

        # Initialization logic
        if not self.initialized:
            self._set_has_inputs(True)
            self._set_initialized(True)

        # No weighting for this price (also avoiding divide by zero)
        if volume == 0.0:
            return

        self._price_volume += price * volume
        self._volume_total += volume
        self.value = self._price_volume / self._volume_total

    cpdef void reset(self) except *:
        """
        Reset the indicator by clearing all stateful values.
        """
        self._reset_base()
        self._day = 0
        self._price_volume = 0.0
        self._volume_total = 0.0
        self.value = 0.0
