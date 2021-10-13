# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.tick_scheme.base import register_tick_scheme


cdef class TieredTickScheme(TickScheme):
    """
    Represents a tick scheme where tick levels change based on price level, such as various financial exchanges.
    """

    def __init__(self, str name, object tiers, int max_ticks_per_tier=100):
        """
        Initialize a new instance of the `Instrument` class.

        Parameters
        ----------
        tiers: List[Tuple(start, stop, step)]
            The tiers for the tick scheme. Should be a list of (start, stop, step) tuples
        """
        self.tiers = self._validate_tiers(tiers)
        self.max_ticks_per_tier = max_ticks_per_tier
        self.ticks = self._build_ticks()
        super().__init__(name=name, min_tick=min(self.ticks), max_tick=max(self.ticks))
        self.tick_count = len(self.ticks)

    @staticmethod
    def _validate_tiers(tiers):
        for x in tiers:
            assert len(x) == 3, "Mappings should be list of tuples like [(start, stop, increment), ...]"
            start, stop, incr = x
            assert start < stop, f"Start should be less than stop (start={start}, stop={stop})"
            assert incr <= start and incr <= stop, f"Increment should be less than start and stop ({start}, {stop}, {incr})"
        return tiers

    cpdef _build_ticks(self):
        """ Expand mappings into the full tick values """
        cdef list all_ticks = []
        for start, stop, step in self.tiers:
            if stop == np.inf:
                stop = start + self.max_ticks_per_tier + 1
            precision = Price.from_str_c(str(step)).precision
            ticks = [Price(value=x, precision=precision) for x in np.arange(start, stop, step)]
            if len(ticks) > self.max_ticks_per_tier:
                print(f"{self.name}: too many ticks for tier ({start=}, {stop=}, {step=}, trimming to {self.max_ticks_per_tier} (from {len(ticks)})")
                ticks = ticks[:self.max_ticks_per_tier]
            all_ticks.extend(ticks)
        return np.asarray(all_ticks)

    cpdef int find_tick_index(self, double value):
        cdef int idx = self.ticks.searchsorted(value)
        print(f"Searching for {value=}, {idx=}, exact?={value == self.ticks[idx].as_double()}")
        if value == self.ticks[idx].as_double():
            return idx
        return idx

    cpdef Price next_ask_tick(self, double value, int n=0):
        Condition.not_negative(n, "n")
        cdef int idx = self.find_tick_index(value)
        print(idx)
        Condition.true(idx + n <= self.tick_count, f"n={n} beyond ask tick bound")
        return self.ticks[idx + n]

    cpdef Price next_bid_tick(self, double value, int n=0):
        Condition.not_negative(n, "n")
        cdef int idx = self.find_tick_index(value)
        Condition.true((idx - n) > 0, f"n={n} beyond bid tick bound")
        if self.ticks[idx].as_double() == value:
            return self.ticks[idx - n]
        return self.ticks[idx - 1 - n]


BetfairTickScheme = TieredTickScheme(
    name="BetfairTickScheme",
    tiers=[
        (1.01, 2, 0.01),
        (2, 3, 0.02),
        (3, 4, 0.05),
        (4, 6, 0.1),
        (6, 10, 0.2),
        (10, 20, 0.5),
        (20, 30, 1),
        (30, 50, 2),
        (50, 100, 5),
        (100, 1000, 10),
    ]
)

TOPIX100TickScheme = TieredTickScheme(
    name="TOPIX100TickScheme",
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
    max_ticks_per_tier=10000,
)

register_tick_scheme(BetfairTickScheme)
register_tick_scheme(TOPIX100TickScheme)
