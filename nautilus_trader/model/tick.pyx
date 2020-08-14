# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import pytz
from cpython.datetime cimport datetime

from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.objects cimport Price, Volume
from nautilus_trader.model.identifiers cimport Symbol


cdef class Tick:
    """
    Represents a single tick in a financial market.
    """

    def __init__(self,
                 Symbol symbol not None,
                 Price bid not None,
                 Price ask not None,
                 Volume bid_size not None,
                 Volume ask_size not None,
                 datetime timestamp not None):
        """
        Initializes a new instance of the Tick class.

        :param symbol: The ticker symbol.
        :param bid: The best bid price.
        :param ask: The best ask price.
        :param bid_size: The bid size.
        :param ask_size: The ask size.
        :param timestamp: The tick timestamp (UTC).
        """
        self.symbol = symbol
        self.bid = bid
        self.ask = ask
        self.bid_size = bid_size
        self.ask_size = ask_size
        self.timestamp = timestamp

    @staticmethod
    cdef Tick from_serializable_string_with_symbol(Symbol symbol, str values):
        """
        Return a tick parsed from the given symbol and values string.

        :param symbol: The tick symbol.
        :param values: The tick values string.
        :return Tick.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.valid_string(values, 'values')

        return Tick._parse(symbol, values.split(',', maxsplit=4))

    @staticmethod
    cdef Tick from_serializable_string(str value):
        """
        Return a tick parsed from the given value string.

        :param value: The tick value string to parse.
        :return Tick.
        """
        Condition.valid_string(value, 'value')

        cdef list pieces = value.split(',', maxsplit=1)

        return Tick._parse(Symbol.from_string(pieces[0]), pieces[1:])

    @staticmethod
    cdef Tick _parse(Symbol symbol, list pieces):
        return Tick(
            symbol,
            Price.from_string(pieces[0]),
            Price.from_string(pieces[1]),
            Volume.from_string(pieces[2]),
            Volume.from_string(pieces[3]),
            datetime.fromtimestamp(long(pieces[4]) / 1000, pytz.utc))

    @staticmethod
    def py_from_serializable_string_with_symbol(Symbol symbol, str values) -> Tick:
        """
        Python wrapper for the from_string_with_symbol method.

        Return a tick parsed from the given symbol and values string.

        :param symbol: The tick symbol.
        :param values: The tick values string.
        :return Tick.
        """
        return Tick.from_serializable_string_with_symbol(symbol, values)

    @staticmethod
    def py_from_serializable_string(str values) -> Tick:
        """
        Python wrapper for the from_string method.

        Return a tick parsed from the given values string.

        :param values: The tick values string.
        :return Tick.
        """
        return Tick.from_serializable_string(values)

    cpdef bint equals(self, Tick other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return (self.symbol.equals(other.symbol) and      # noqa (W504 - easier to read)
                self.bid.equals(other.bid) and            # noqa (W504 - easier to read)
                self.ask.equals(other.ask) and            # noqa (W504 - easier to read)
                self.bid_size.equals(other.bid_size) and  # noqa (W504 - easier to read)
                self.ask_size.equals(other.ask_size) and  # noqa (W504 - easier to read)
                self.timestamp == other.timestamp)        # noqa (W504 - easier to read)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        :return: str.
        """
        return (f"{self.symbol.to_string()},"
                f"{self.bid.to_string()},"
                f"{self.ask.to_string()},"
                f"{self.bid_size.to_string()},"
                f"{self.ask_size.to_string()},"
                f"{format_iso8601(self.timestamp)}")

    cpdef str to_serializable_string(self):
        """
        Return the serializable string representation of this object.

        :return: str.
        """
        return (f"{self.bid.to_string()},"
                f"{self.ask.to_string()},"
                f"{self.bid_size.to_string()},"
                f"{self.ask_size.to_string()},"
                f"{long(self.timestamp.timestamp())}")

    def __eq__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.
        Note: The equality is based on the ticks timestamp only.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.
        Note: The equality is based on the ticks timestamp only.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.timestamp)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.to_string()}) object at {id(self)}>"
