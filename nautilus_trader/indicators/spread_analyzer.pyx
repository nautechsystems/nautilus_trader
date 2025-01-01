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

from collections import deque

import numpy as np

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.stats cimport fast_mean_iterated
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price


cdef class SpreadAnalyzer(Indicator):
    """
    Provides various spread analysis metrics.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the tick updates.
    capacity : int
        The max length for the internal `QuoteTick` deque (determines averages).

    Raises
    ------
    ValueError
        If `capacity` is not positive (> 0).
    """

    def __init__(self, InstrumentId instrument_id not None, int capacity) -> None:
        Condition.positive_int(capacity, "capacity")
        super().__init__(params=[instrument_id, capacity])

        self.instrument_id = instrument_id
        self.capacity = capacity
        self._spreads = deque(maxlen=capacity)

        self.current = 0
        self.average = 0

    cpdef void handle_quote_tick(self, QuoteTick tick):
        """
        Update the analyzer with the given quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick for the update.

        Raises
        ------
        ValueError
            If `tick.instrument_id` does not equal the analyzers instrument ID.

        """
        Condition.not_none(tick, "tick")
        Condition.equal(self.instrument_id, tick.instrument_id, "instrument_id", "tick.instrument_id")

        # Check initialization
        if not self.initialized:
            self._set_has_inputs(True)
            if len(self._spreads) == self.capacity:
                self._set_initialized(True)

        cdef double bid = Price.raw_to_f64_c(tick._mem.bid_price.raw)
        cdef double ask = Price.raw_to_f64_c(tick._mem.ask_price.raw)
        cdef double spread = ask - bid

        self.current = spread
        self._spreads.append(spread)

        # Update average spread
        self.average = fast_mean_iterated(
            values=np.asarray(self._spreads, dtype=np.float64),
            next_value=spread,
            current_value=self.average,
            expected_length=self.capacity,
            drop_left=False,
        )

    cpdef void _reset(self):
        self._spreads.clear()
        self.current = 0
        self.average = 0
