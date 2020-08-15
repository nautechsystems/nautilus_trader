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
from nautilus_trader.model.c_enums.tick_spec cimport TickSpecification
from nautilus_trader.model.c_enums.tick_spec cimport tick_spec_to_string, tick_spec_from_string
from nautilus_trader.model.c_enums.maker cimport Maker, maker_from_string, maker_to_string
from nautilus_trader.model.objects cimport Price, Quantity
from nautilus_trader.model.identifiers cimport Symbol, Venue, MatchId


cdef class TickType:
    """
    Represents a financial market symbol and tick specification.
    """

    def __init__(self,
                 Symbol symbol not None,
                 TickSpecification tick_spec):
        """
        Initializes a new instance of the TickType class.

        Parameters
        ----------
        tick_spec : TickSpecification
            The tick specification.
        symbol : Symbol
            The ticker symbol.
        """
        Condition.not_equal(tick_spec, TickSpecification.UNDEFINED, 'tick_spec', 'UNDEFINED')

        self.symbol = symbol
        self.spec = tick_spec

    @staticmethod
    cdef TickType from_string(str value):
        """
        Return a tick type parsed from the given string.

        :param value: The tick type string to parse.
        :return TickType.
        """
        Condition.valid_string(value, 'value')

        cdef list pieces = value.split('-', maxsplit=1)
        cdef list symbol_pieces = pieces[0].split('.', maxsplit=1)
        cdef Symbol symbol = Symbol(symbol_pieces[0], Venue(symbol_pieces[1]))

        return TickType(symbol, tick_spec_from_string(pieces[1]))

    cdef str spec_string(self):
        """
        Return the tick specification as a string.
        
        :return str.
        """
        return tick_spec_to_string(self.spec)

    cpdef bint equals(self, TickType other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.symbol.equals(other.symbol) and self.spec == other.spec

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        :return: str.
        """
        return f"{self.symbol.to_string()}-{self.spec_string()}"

    def __eq__(self, TickType other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, TickType other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash((self.symbol, self.spec))

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


cdef class Tick:
    """
    The base class for all ticks.
    """

    def __init__(self,
                 Symbol symbol not None,
                 TickSpecification tick_spec,
                 datetime timestamp not None):
        """
        Initializes a new instance of the Tick class.

        Parameters
        ----------
        tick_spec : TickSpecification
            The tick specification.
        symbol : Symbol
            The ticker symbol.
        timestamp : datetime
            The tick timestamp (UTC).

        """
        Condition.not_equal(tick_spec, TickSpecification.UNDEFINED, 'tick_type', 'UNDEFINED')

        self.symbol = symbol
        self.spec = tick_spec
        self.timestamp = timestamp

    cpdef TickType get_type(self):
        """
        Return the tick type from this ticks internal data.
        
        return TickType.
        """
        return TickType(self.symbol, self.spec)

    cpdef bint equals(self, Tick other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef str spec_string(self):
        """
        Return the tick specification as a string.
    
        :return str.
        """
        return tick_spec_to_string(self.spec)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        :return: str.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef str to_serializable_string(self):
        """
        Return the serializable string representation of this object.

        :return: str.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

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
        Note: The hash is based on the ticks timestamp only.

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


cdef class QuoteTick(Tick):
    """
    Represents a single quote tick in a financial market.
    """

    def __init__(self,
                 Symbol symbol not None,
                 Price bid not None,
                 Price ask not None,
                 Quantity bid_size not None,
                 Quantity ask_size not None,
                 datetime timestamp not None):
        """
        Initializes a new instance of the QuoteTick class.

        Parameters
        ----------
        symbol : Symbol
            The ticker symbol.
        bid : Price
            The best bid price.
        ask : Price
            The best ask price.
        bid_size : Quantity
            The size at the best bid.
        ask_size : Quantity
            The size at the best ask.
        timestamp : datetime
            The tick timestamp (UTC).

        """
        super().__init__(symbol, TickSpecification.QUOTE, timestamp)

        self.bid = bid
        self.ask = ask
        self.bid_size = bid_size
        self.ask_size = ask_size

    @staticmethod
    cdef QuoteTick from_serializable_string(Symbol symbol, str values):
        """
        Return a tick parsed from the given symbol and values string.

        :param symbol: The tick symbol.
        :param values: The tick values string.
        :return Tick.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.valid_string(values, 'values')

        cdef list pieces = values.split(',', maxsplit=4)

        return QuoteTick(
            symbol,
            Price.from_string(pieces[0]),
            Price.from_string(pieces[1]),
            Quantity.from_string(pieces[2]),
            Quantity.from_string(pieces[3]),
            datetime.fromtimestamp(long(pieces[4]) / 1000, pytz.utc))

    @staticmethod
    def py_from_serializable_string(Symbol symbol, str values):
        """
        Python wrapper for the from_string_with_symbol method.

        Return a tick parsed from the given symbol and values string.

        :param symbol: The tick symbol.
        :param values: The tick values string.
        :return Tick.
        """
        return QuoteTick.from_serializable_string(symbol, values)

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


cdef class TradeTick(Tick):
    """
    Represents a single trade tick in a financial market.
    """

    def __init__(self,
                 Symbol symbol not None,
                 Price price not None,
                 Quantity size not None,
                 Maker maker,
                 MatchId match_id not None,
                 datetime timestamp not None):
        """
        Initializes a new instance of the TradeTick class.

        Parameters
        ----------
        symbol : Symbol
            The ticker symbol.
        price : Price
            The price of the trade.
        size : Quantity
            The size of the trade.
        maker : Maker
            The trade maker.
        match_id : MatchId
            The unique identifier for the trade match.
        timestamp : datetime
            The tick timestamp (UTC).

        """
        super().__init__(symbol, TickSpecification.TRADE, timestamp)

        self.price = price
        self.size = size
        self.maker = maker
        self.match_id = match_id

    @staticmethod
    cdef TradeTick from_serializable_string(Symbol symbol, str values):
        """
        Return a tick parsed from the given symbol and values string.

        :param symbol: The tick symbol.
        :param values: The tick values string.
        :return Tick.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.valid_string(values, 'values')

        cdef list pieces = values.split(',', maxsplit=4)

        return TradeTick(
            symbol,
            Price.from_string(pieces[0]),
            Quantity.from_string(pieces[1]),
            maker_from_string(pieces[2]),
            MatchId(pieces[3]),
            datetime.fromtimestamp(long(pieces[4]) / 1000, pytz.utc))

    @staticmethod
    def py_from_serializable_string(Symbol symbol, str values):
        """
        Python wrapper for the from_string_with_symbol method.

        Return a tick parsed from the given symbol and values string.

        :param symbol: The tick symbol.
        :param values: The tick values string.
        :return Tick.
        """
        return TradeTick.from_serializable_string(symbol, values)

    cpdef bint equals(self, Tick other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return (self.symbol.equals(other.symbol) and      # noqa (W504 - easier to read)
                self.price.equals(other.price) and        # noqa (W504 - easier to read)
                self.size.equals(other.size) and          # noqa (W504 - easier to read)
                self.maker == other.maker and             # noqa (W504 - easier to read)
                self.match_id.equals(other.match_id) and  # noqa (W504 - easier to read)
                self.timestamp == other.timestamp)        # noqa (W504 - easier to read)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        :return: str.
        """
        return (f"{self.symbol.to_string()},"
                f"{self.price.to_string()},"
                f"{self.size.to_string()},"
                f"{maker_to_string(self.maker)},"
                f"{self.match_id.to_string()},"
                f"{format_iso8601(self.timestamp)}")

    cpdef str to_serializable_string(self):
        """
        Return the serializable string representation of this object.

        :return: str.
        """
        return (f"{self.price.to_string()},"
                f"{self.size.to_string()},"
                f"{maker_to_string(self.maker)},"
                f"{self.match_id.to_string()},"
                f"{long(self.timestamp.timestamp())}")
