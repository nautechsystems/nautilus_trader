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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.objects cimport Price


cdef class TickScheme:
    """
    Represents an instrument tick scheme.

    Maps the valid prices available for an instrument.

    Parameters
    ----------
    name : str
        The name of the tick scheme.
    min_tick : Price
        The minimum possible tick `Price`.
    max_tick: Price
        The maximum possible tick `Price`.

    Raises
    ------
    ValueError
        If `name` is not a valid string.
    """

    def __init__(
        self,
        str name not None,
        Price min_tick not None,
        Price max_tick not None,
    ):
        Condition.valid_string(name, "name")

        self.name = name
        self.min_price = min_tick
        self.max_price = max_tick

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
        raise NotImplementedError()  # pragma: no cover

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
        raise NotImplementedError()  # pragma: no cover


cdef inline double _round_base(double value, double base):
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


cdef dict TICK_SCHEMES = {}  # type: dict[str, TickScheme]

cpdef void register_tick_scheme(TickScheme tick_scheme):
    Condition.not_none(tick_scheme, "tick_scheme")

    global TICK_SCHEMES
    Condition.not_in(tick_scheme.name, TICK_SCHEMES, "name", "TICK_SCHEMES")
    TICK_SCHEMES[tick_scheme.name] = tick_scheme


cpdef TickScheme get_tick_scheme(str name):
    Condition.valid_string(name, "name")
    Condition.is_in(name, TICK_SCHEMES, "name", "TICK_SCHEMES")
    return TICK_SCHEMES[name]


cpdef list list_tick_schemes():
    return list(TICK_SCHEMES)
