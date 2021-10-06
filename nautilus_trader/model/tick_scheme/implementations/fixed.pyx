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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick_scheme.base cimport TickScheme

from nautilus_trader.model.tick_scheme.base import register_tick_scheme


cdef class FixedTickScheme(TickScheme):
    """
    Represents a Fixed precision tick scheme such as Forex or Crypto.
    """

    def __init__(
            self,
            int price_precision,
            Price min_tick,
            Price max_tick,
    ):
        """
        Initialize a new instance of the `Instrument` class.

        Parameters
        ----------
        price_precision: int
            The instrument price precision
        min_tick : Price
            The minimum possible tick `Price`
        max_tick: Price
            The maximum possible tick `Price`

        """

        self.price_precision = price_precision
        self.min_tick = min_tick
        self.max_tick = max_tick
        self.increment = Price.from_str_c('1'.zfill(price_precision))

    def next_ask_tick(self, price):
        """
        For a given price, return the next ask (higher) price on the ladder

        :param price: The relative price
        :return: Price
        """
        cdef int idx
        if price >= self.max_tick:
            return None
        idx = self.ticks.searchsorted(price)
        if price in self.ticks:
            return self.ticks[idx + 1]
        else:
            return self.ticks[idx]

    cpdef Price next_bid_tick(self, Price price):
        """
        For a given price, return the next bid (lower)price on the ladder

        :param price: The relative price
        :return: Price
        """
        cdef int idx
        if price <= self.min_tick:
            return None
        idx = self.ticks.searchsorted(price)
        if price in self.ticks:
            return self.ticks[idx - 1]
        else:
            return self.ticks[idx - 1]


# Most FOREX pairs
FixedTickScheme4Decimal = FixedTickScheme(
    price_precision=4,
    min_tick=Price.from_str_c("0.0001"),
    max_tick=Price.from_str_c("9.9999"),
)

# JPY denominated FOREX pairs
FixedTickScheme2Decimal = FixedTickScheme(
    price_precision=2,
    min_tick=Price.from_str_c("0.01"),
    max_tick=Price.from_str_c("999.99"),
)

register_tick_scheme("FixedTickScheme4Decimal", FixedTickScheme4Decimal)
register_tick_scheme("FixedTickScheme2Decimal", FixedTickScheme2Decimal)
