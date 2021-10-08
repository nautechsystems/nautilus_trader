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
from numpy cimport ndarray
import numpy as np

from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick_scheme.base cimport TickScheme

from nautilus_trader.model.tick_scheme.base import register_tick_scheme
from nautilus_trader.model.tick_scheme.base import round_down
from nautilus_trader.model.tick_scheme.base import round_up


cdef class TieredTickScheme(TickScheme):
    """
    Represents a tick scheme where tick levels change based on price level, such as various financial exchanges.
    """

    def __init__(self, object tiers):
        """
        Initialize a new instance of the `Instrument` class.

        Parameters
        ----------
        tiers: List[Tuple(start, stop, step)]
            The tiers for the tick scheme. Should be a list of (start, stop, step) tuples
        """
        self.tiers = self._validate_tiers(tiers)
        self.ticks: ndarray = self.build_ticks(tiers)
        self.min_tick = self.ticks[0]
        self.max_tick = self.ticks[-1]

    @staticmethod
    def _validate_tiers(tiers):
        for x in tiers:
            assert len(x) == 3, "Mappings should be list of tuples like [(start, stop, increment), ...]"
            start, stop, incr = x
            assert start < stop, f"Start should be less than stop (start={start}, stop={stop})"
            assert incr <= start and incr <= stop, f"Increment should be less than start and stop ({start}, {stop}, {incr})"
        return tiers

    cdef ndarray build_ticks(self, list tiers):
        """ Expand mappings in the full tick values """
        cdef list ticks = []
        for start, end, step in tiers:
            example = Price.from_str(str(step))
            ticks.extend([
                Price(value=x, precision=example.precision)
                for x in np.arange(start, end, step)
            ])
        return np.asarray(ticks)

    cpdef Price next_ask_tick(self, double value):
        """
        For a given price, return the next ask (higher) price on the ladder

        :param value: The price
        :return: Price
        """
        cdef int idx
        if value >= self.max_tick:
            return None
        idx = self.ticks.searchsorted(value)
        if value in self.ticks:
            return self.ticks[idx + 1]
        else:
            return self.ticks[idx]

    cpdef Price next_bid_tick(self, double value):
        """
        For a given price, return the next bid (lower)price on the ladder

        :param value: The price
        :return: Price
        """
        cdef int idx
        if value >= self.max_tick:
            return None
        idx = self.ticks.searchsorted(value)
        if value in self.ticks:
            return self.ticks[idx + 1]
        else:
            return self.ticks[idx]

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
        (0, 1_000, 0.1),
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
