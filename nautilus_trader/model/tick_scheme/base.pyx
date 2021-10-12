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


cdef class TickScheme:
    """
    Represents a instrument tick scheme, mapping the prices available for an instrument
    """

    def __init__(self, Price min_tick, Price max_tick):
        """
        Initialize a new instance of the `TickScheme` class.

        Parameters
        ----------
        min_tick : Price
            The minimum possible tick `Price`
        max_tick: Price
            The maximum possible tick `Price`
        """

    cpdef Price nearest_ask_tick(self, double price):
        """
        For a given `price`, return the nearest ask (higher)  tick (simply returning `price` if it is a valid tick).

        :param price: The price
        :return: Price
        """
        raise NotImplementedError

    cpdef Price next_ask_tick(self, double price, int n=0):
        """
        Return the `Price` `n` ask ticks away from `price`.

        If a given price is between two ticks, n=0 will find the nearest ask tick.

        :param price: The reference price
        :param n: The number of ticks to move
        :return: Price
        """
        raise NotImplementedError

    cpdef Price nearest_bid_tick(self, double price):
        """
        For a given `price`, return the nearest bid (lower) tick (simply returning `price` if it is a valid tick).

        :param price: The price
        :return: Price
        """
        raise NotImplementedError

    cpdef Price next_bid_tick(self, double price, int n=0):
        """
        Return the `Price` `n` bid ticks away from `price`.

        If a given price is between two ticks, n=0 will find the nearest bid tick.

        :param price: The reference price
        :param n: The number of ticks to move
        :return: Price
        """
        raise NotImplementedError


TICK_SCHEMES = {}

cpdef void register_tick_scheme(str name, tick_scheme: TickScheme):
    global TICK_SCHEMES
    Condition.not_in(name, TICK_SCHEMES, "name", "TICK_SCHEMES")
    TICK_SCHEMES[name] = tick_scheme


cpdef TickScheme get_tick_scheme(str name):
    Condition.is_in(name, TICK_SCHEMES, "name", "TICK_SCHEMES")
    return TICK_SCHEMES[name]


cpdef list list_tick_schemes():
    return list(TICK_SCHEMES)


cdef double _round_base(double value, double base):
    """
    >>> _round_base(0.72775, 0.0001)
    0.7277
    """
    return int(value / base) * base


cpdef double round_down(double value, double base):
    """
    Returns a value rounded down to a specific number of decimal places.
    """
    return _round_base(value=value, base=base)


cpdef double round_up(double value, double base):
    """
    Returns a value rounded down to a specific number of decimal places.
    """
    return _round_base(value=value, base=base) + base
