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

from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick_scheme.base cimport TickScheme
from nautilus_trader.model.tick_scheme.base cimport register_tick_scheme
from nautilus_trader.model.tick_scheme.base cimport round_down
from nautilus_trader.model.tick_scheme.base cimport round_up


cdef class FixedTickScheme(TickScheme):
    """
    Represents a fixed precision tick scheme such as for Forex or Crypto.

    Parameters
    ----------
    name : str
        The name of the tick scheme.
    price_precision: int
        The instrument price precision.
    min_tick : Price
        The minimum possible tick `Price`.
    max_tick: Price
        The maximum possible tick `Price`.
    increment : float, optional
        The tick increment.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    """

    def __init__(
        self,
        str name not None,
        int price_precision,
        Price min_tick not None,
        Price max_tick not None,
        increment: float | None = None,
    ):
        super().__init__(name=name, min_tick=min_tick, max_tick=max_tick)
        self.price_precision = price_precision
        self.increment = Price.from_str(str(increment or "0." + "1".zfill(price_precision)))
        self._increment = self.increment.as_f64_c()

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
        if value > self.max_price:
            return None
        cdef double rounded = round_up(value=value, base=self._increment) + (n * self._increment)
        return Price(rounded, precision=self.price_precision)

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
        if value < self.min_price:
            return None
        cdef double rounded = round_down(value=value, base=self._increment) - (n * self._increment)
        return Price(rounded, precision=self.price_precision)


# Most FOREX pairs
FOREX_5DECIMAL_TICK_SCHEME = FixedTickScheme(
    name="FOREX_5DECIMAL",
    price_precision=5,
    min_tick=Price.from_str_c("0.00001"),
    max_tick=Price.from_str_c("9.99999"),
)

# JPY denominated FOREX pairs
FOREX_3DECIMAL_TICK_SCHEME = FixedTickScheme(
    name="FOREX_3DECIMAL",
    price_precision=3,
    min_tick=Price.from_str_c("0.001"),
    max_tick=Price.from_str_c("999.999"),
)
