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

import pandas as pd
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport Bar


cdef class VolumeWeightedAveragePrice(Indicator):
    """
    An indicator which calculates the volume weighted average price for the day.
    """

    def __init__(self):
        super().__init__(params=[])

        self._day = 0
        self._price_volume = 0
        self._volume_total = 0
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

        self.update_raw(
            (
                bar.close.as_double() +
                bar.high.as_double() +
                bar.low.as_double()
            ) / 3.0,
            bar.volume.as_double(),
            pd.Timestamp(bar.ts_init, tz="UTC"),
        )

    cpdef void update_raw(
        self,
        double price,
        double volume,
        datetime timestamp,
    ):
        """
        Update the indicator with the given raw values.

        Parameters
        ----------
        price : double
            The update price.
        volume : double
            The update volume.
        timestamp : datetime
            The current timestamp.

        """
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
        if volume == 0:
            return

        self._price_volume += price * volume
        self._volume_total += volume
        self.value = self._price_volume / self._volume_total

    cpdef void _reset(self):
        self._day = 0
        self._price_volume = 0
        self._volume_total = 0
        self.value = 0
