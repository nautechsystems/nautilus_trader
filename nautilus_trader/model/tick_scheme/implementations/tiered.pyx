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

from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick_scheme.base cimport TickScheme

from nautilus_trader.core.correctness import Condition
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
        Condition.type(tiers, list, "tiers")
        [Condition.type(t, tuple, "tier") for t in tiers]
        self.tiers = tiers

    cpdef Price next_ask_tick(self, double value):
        """
        For a given price, return the next ask (higher) price on the ladder

        :param value: The price
        :return: Price
        """
        return round_up(value=value)

    cpdef Price next_bid_tick(self, double value):
        """
        For a given price, return the next bid (lower)price on the ladder

        :param value: The price
        :return: Price
        """
        return round_down(value=value)


betfair_tick_scheme = TieredTickScheme(
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

register_tick_scheme("BetfairTickScheme", betfair_tick_scheme)
