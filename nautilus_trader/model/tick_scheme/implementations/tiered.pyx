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

import numpy as np

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick_scheme.base cimport TickScheme


cdef class TieredTickScheme(TickScheme):
    """
    Represents a tick scheme where tick levels change based on price level, such as various financial exchanges.

    Parameters
    ----------
    name : str
        The name of the tick scheme.
    tiers : list[tuple(start, stop, step)]
        The tiers for the tick scheme. Should be a list of (start, stop, step) tuples.
    max_ticks_per_tier : int, default 100
        The maximum number of ticks per tier.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    """

    def __init__(
        self,
        str name not None,
        list tiers not None,
        int price_precision,
        int max_ticks_per_tier=100,
    ):
        self.price_precision = price_precision
        self.tiers = self._validate_tiers(tiers)
        self.max_ticks_per_tier = max_ticks_per_tier
        self.ticks = self._build_ticks()
        super().__init__(name, min(self.ticks), max(self.ticks))
        self.tick_count = len(self.ticks)

    @staticmethod
    def _validate_tiers(list tiers):
        for x in tiers:
            assert len(x) == 3, "Mappings should be list of tuples like [(start, stop, increment), ...]"
            start, stop, incr = x
            assert start < stop, f"Start should be less than stop (start={start}, stop={stop})"
            assert incr <= start and incr <= stop, f"Increment should be less than start and stop ({start}, {stop}, {incr})"
        return tiers

    cpdef _build_ticks(self):
        # Expand mappings into the full tick values
        cdef list all_ticks = []
        for start, stop, step in self.tiers:
            if stop == np.inf:
                stop = start + ((self.max_ticks_per_tier + 1) * step)
            ticks = [Price(x, self.price_precision) for x in np.arange(start, stop, step)]
            if len(ticks) > self.max_ticks_per_tier+1:
                print(f"{self.name}: too many ticks for tier ({start=}, {stop=}, {step=}, trimming to {self.max_ticks_per_tier} (from {len(ticks)})")
                ticks = ticks[:self.max_ticks_per_tier]
            all_ticks.extend(ticks)
        return np.asarray(all_ticks)

    cpdef int find_tick_index(self, double value):
        cdef int idx = self.ticks.searchsorted(value)
        cdef double prev_value = self.ticks[idx - 1].as_double()
        # print(f"Searching for {value=}, {idx=}, {prev_value=}, exact?={value == prev_value}")
        if value == prev_value:
            return idx - 1
        return idx

    cpdef Price next_ask_price(self, double value, int n=0):
        """
        Return the price `n` ask ticks away from value.

        If a given price is between two ticks, n=0 will find the nearest ask tick.

        Parameters
        ----------
        value : double
            The reference value.
        n : int, default 0
            The number of ticks to move.

        Returns
        -------
        Price

        """
        Condition.not_negative(n, "n")
        cdef int idx = self.find_tick_index(value)
        Condition.is_true(idx + n <= self.tick_count, f"n={n} beyond ask tick bound")
        return self.ticks[idx + n]

    cpdef Price next_bid_price(self, double value, int n=0):
        """
        Return the price `n` bid ticks away from value.

        If a given price is between two ticks, n=0 will find the nearest bid tick.

        Parameters
        ----------
        value : double
            The reference value.
        n : int, default 0
            The number of ticks to move.

        Returns
        -------
        Price

        """
        Condition.not_negative(n, "n")
        cdef int idx = self.find_tick_index(value)
        Condition.is_true((idx - n) > 0, f"n={n} beyond bid tick bound")
        if self.ticks[idx].as_double() == value:
            return self.ticks[idx - n]
        return self.ticks[idx - 1 - n]


TOPIX100_TICK_SCHEME = TieredTickScheme(
    name="TOPIX100",
    tiers=[
        (0.1, 1_000, 0.1),
        (1_000, 3_000, 0.5),
        (3_000, 10_000, 1),
        (10_000, 30_000, 5),
        (30_000, 100_000, 10),
        (100_000, 300_000, 50),
        (300_000, 1_000_000, 100),
        (1_000_000, 3_000_000, 500),
        (3_000_000, 10_000_000, 1_000),
        (10_000_000, 30_000_000, 5_000),
        (30_000_000, np.inf, 10_000),
    ],
    price_precision=4,
    max_ticks_per_tier=10_000,
)
