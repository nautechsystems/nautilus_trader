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

from nautilus_trader.model.tick_scheme.base import register_tick_scheme
from nautilus_trader.model.tick_scheme.base import round_down
from nautilus_trader.model.tick_scheme.base import round_up


cdef class FixedTickScheme(TickScheme):
    """
    Represents a Fixed precision tick scheme such as Forex or Crypto.
    """

    def __init__(
            self,
            str name,
            int price_precision,
            Price min_tick,
            Price max_tick,
            increment=None,
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
        super().__init__(name=name, min_tick=min_tick, max_tick=max_tick)
        self.price_precision = price_precision
        self.increment = Price.from_str(str(increment or "0." + "1".zfill(price_precision)))

    cpdef Price next_ask_tick(self, double value, int n=0):
        """
        Return the `Price` `n` bid ticks away from `price`.

        If a given price is between two ticks, n=0 will find the nearest bid tick.

        :param value: The reference price
        :param n: The number of ticks to move
        :return: Price
        """
        if value > self.max_tick:
            return None
        cdef double base = self.increment.as_double()
        cdef double rounded = round_up(value=value, base=base) + (n * base)
        return Price(rounded, precision=self.price_precision)

    cpdef Price next_bid_tick(self, double value, int n=0):
        """
        Return the `Price` `n` bid ticks away from `price`.

        If a given price is between two ticks, n=0 will find the nearest bid tick.

        :param value: The reference price
        :param n: The number of ticks to move
        :return: Price
        """
        if value < self.min_tick:
            return None
        cdef double base = self.increment.as_double()
        cdef double rounded = round_down(value=value, base=base) - (n * base)
        return Price(rounded, precision=self.price_precision)


# Most FOREX pairs
FOREX_5DECIMAL_TICK_SCHEME = FixedTickScheme(
    name="FOREX_5DECIMAL",
    price_precision=5,
    min_tick=Price.from_str_c("0.00001"),
    max_tick=Price.from_str_c("9.99999"),
)
register_tick_scheme(FOREX_5DECIMAL_TICK_SCHEME)

# JPY denominated FOREX pairs
FOREX_3DECIMAL_TICK_SCHEME = FixedTickScheme(
    name="FOREX_3DECIMAL",
    price_precision=3,
    min_tick=Price.from_str_c("0.001"),
    max_tick=Price.from_str_c("999.999"),
)
register_tick_scheme(FOREX_3DECIMAL_TICK_SCHEME)
