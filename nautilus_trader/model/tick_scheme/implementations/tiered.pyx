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
cimport numpy as np

import numpy as np

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick_scheme.base cimport TickScheme

from nautilus_trader.model.tick_scheme.base import register_tick_scheme
from nautilus_trader.model.tick_scheme.base import round_down
from nautilus_trader.model.tick_scheme.base import round_up


cdef class TieredTickScheme(TickScheme):
    """
    Represents a tick scheme where tick levels change based on price level, such as various financial exchanges.
    """

    def __init__(self, object tiers, bint build_ticks=True):
        """
        Initialize a new instance of the `Instrument` class.

        Parameters
        ----------
        tiers: List[Tuple(start, stop, step)]
            The tiers for the tick scheme. Should be a list of (start, stop, step) tuples
        """
        self.tiers = self._validate_tiers(tiers)
        self.min_tick = Price.from_str(str(tiers[0][0]))
        self.max_tick = Price.from_str(str(tiers[-1][1]))
        self.boundaries: np.ndarray[np.float_t] = np.asarray([0] + [t[0] for t in tiers])
        self.bases: np.ndarray[np.int_t] = np.asarray([np.nan] + [t[2] for t in tiers] + [tiers[-1][2]])
        self.precisions: np.ndarray[np.int_t] = np.asarray([0] + [Price.from_str(str(b)).precision for b in self.boundaries])

    @staticmethod
    def _validate_tiers(tiers):
        for x in tiers:
            assert len(x) == 3, "Mappings should be list of tuples like [(start, stop, increment), ...]"
            start, stop, incr = x
            assert start < stop, f"Start should be less than stop (start={start}, stop={stop})"
            assert incr <= start and incr <= stop, f"Increment should be less than start and stop ({start}, {stop}, {incr})"
        return tiers

    @staticmethod
    def _build_ticks(tiers):
        """ Expand mappings into the full tick values """
        cdef list ticks = []
        for start, end, step in tiers:
            precision = Price(str(step)).precision
            ticks.extend([Price(value=x, precision=precision) for x in np.arange(start, end, step)])
        return np.asarray(ticks)

    cpdef int get_boundaries_idx(self, double value):
        # Check for exact value in boundaries array
        cdef np.ndarray existing = np.where(self.boundaries == value)[0]
        if existing.size > 0:
            return existing[0]

        # Else, find position between boundaries
        cdef int base_idx = self.boundaries.searchsorted(value)
        if base_idx != 0:
            base_idx -= 1
        Condition.in_range(value, self.min_tick, self.max_tick, "value")
        return base_idx

    cpdef Price nearest_ask_tick(self, double value):
        """
        For a given price, return the next ask (higher) price on the ladder

        :param value: The price
        :return: Price
        """
        cdef int base_idx = self.get_boundaries_idx(value=value)
        cdef double rounded = round_up(value=value, base=self.bases[base_idx])
        return Price(rounded, precision=self.precisions[base_idx])

    cpdef Price nearest_bid_tick(self, double value):
        """
        For a given price, return the next bid (lower)price on the ladder

        :param value: The price
        :return: Price
        """
        cdef int base_idx = self.get_boundaries_idx(value=value)
        cdef double rounded = round_down(value=value, base=self.bases[base_idx])
        return Price(rounded, precision=self.precisions[base_idx])


BetfairTickScheme = TieredTickScheme(
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
    ]
)

register_tick_scheme("BetfairTickScheme", BetfairTickScheme)
register_tick_scheme("TOPIX100TickScheme", TOPIX100TickScheme)
