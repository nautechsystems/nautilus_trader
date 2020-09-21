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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.model.c_enums.maker cimport Maker
from nautilus_trader.model.c_enums.maker cimport maker_from_string
from nautilus_trader.model.c_enums.maker cimport maker_to_string
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport price_type_to_string
from nautilus_trader.model.identifiers cimport MatchId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Tick:
    """
    The base class for all ticks.
    """

    def __init__(self,
                 Symbol symbol not None,
                 datetime timestamp not None):
        """
        Initialize a new instance of the Tick class.

        Parameters
        ----------
        symbol : Symbol
            The ticker symbol.
        timestamp : datetime
            The tick timestamp (UTC).

        """
        self.symbol = symbol
        self.timestamp = timestamp

    cpdef bint equals(self, Tick other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef str to_string(self):
        """
        Returns a string representation of this object.

        Returns
        -------
        str

        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef str to_serializable_string(self):
        """
        Return a serializable string representation of this object.

        Returns
        -------
        str

        """
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    def __eq__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : Tick
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.equals(other)

    def __ne__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : Tick
            The other object to equate.

        Returns
        -------
        bool

        """
        return not self.equals(other)

    def __lt__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        Parameters
        ----------
        other : Tick
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.timestamp < other.timestamp

    def __le__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given object.

        Parameters
        ----------
        other : Tick
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.timestamp <= other.timestamp

    def __gt__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        Parameters
        ----------
        other : Tick
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.timestamp > other.timestamp

    def __ge__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        Parameters
        ----------
        other : Tick
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.timestamp >= other.timestamp

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        Notes
        -----
        The hash is based on the ticks timestamp only.

        Returns
        -------
        int

        """
        return hash(self.timestamp)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

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
        Initialize a new instance of the QuoteTick class.

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
        super().__init__(symbol, timestamp)

        self.bid = bid
        self.ask = ask
        self.bid_size = bid_size
        self.ask_size = ask_size

    cpdef Price extract_price(self, PriceType price_type):
        """
        Extract the price for the given price type.

        Parameters
        ----------
        price_type : PriceType
            The price type to extraction.

        Returns
        -------
        Price

        """
        if price_type == PriceType.MID:
            return Price((self.bid.as_double() + self.ask.as_double()) / 2, self.bid.precision + 1)
        elif price_type == PriceType.BID:
            return self.bid
        elif price_type == PriceType.ASK:
            return self.ask
        else:
            raise ValueError(f"Cannot extract with PriceType {price_type_to_string(price_type)}")

    cpdef Quantity extract_volume(self, PriceType price_type):
        """
        Extract the volume for the given price type.

        Parameters
        ----------
        price_type : PriceType
            The price type for extraction.

        Returns
        -------
        Quantity

        """
        if price_type == PriceType.MID:
            return Quantity(self.bid_size.as_double() + self.ask_size.as_double())
        elif price_type == PriceType.BID:
            return self.bid_size
        elif price_type == PriceType.ASK:
            return self.ask_size
        else:
            raise ValueError(f"Cannot extract with PriceType {price_type_to_string(price_type)}")

    @staticmethod
    cdef QuoteTick from_serializable_string(Symbol symbol, str values):
        """
        Return a tick parsed from the given symbol and values string.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol.
        values : str
            The tick values string.

        Returns
        -------
        Tick

        Raises
        ------
        ValueError
            If values is not a valid string.

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
        Return a tick parsed from the given symbol and values string.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol.
        values : str
            The tick values string.

        Returns
        -------
        Tick

        Raises
        ------
        ValueError
            If values is not a valid string.

        """
        return QuoteTick.from_serializable_string(symbol, values)

    cpdef bint equals(self, Tick other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return (self.symbol.equals(other.symbol) and      # noqa (W504 - easier to read)
                self.bid.equals(other.bid) and            # noqa (W504 - easier to read)
                self.ask.equals(other.ask) and            # noqa (W504 - easier to read)
                self.bid_size.equals(other.bid_size) and  # noqa (W504 - easier to read)
                self.ask_size.equals(other.ask_size) and  # noqa (W504 - easier to read)
                self.timestamp == other.timestamp)        # noqa (W504 - easier to read)

    cpdef str to_string(self):
        """
        Returns a string representation of the object.

        Returns
        -------
        str

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

        Returns
        -------
        str

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
        Initialize a new instance of the TradeTick class.

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
        super().__init__(symbol, timestamp)

        self.price = price
        self.size = size
        self.maker = maker
        self.match_id = match_id

    @staticmethod
    cdef TradeTick from_serializable_string(Symbol symbol, str values):
        """
        Return a tick parsed from the given symbol and values string.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol.
        values : str
            The tick values string.

        Returns
        -------
        Tick

        Raises
        ------
        ValueError
            If values is not a valid string.

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
        Return a tick parsed from the given symbol and values string.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol.
        values : str
            The tick values string.

        Returns
        -------
        Tick

        Raises
        ------
        ValueError
            If values is not a valid string.

        """
        return TradeTick.from_serializable_string(symbol, values)

    cpdef bint equals(self, Tick other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

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

        Returns
        -------
        str

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

        Returns
        -------
        str

        """
        return (f"{self.price.to_string()},"
                f"{self.size.to_string()},"
                f"{maker_to_string(self.maker)},"
                f"{self.match_id.to_string()},"
                f"{long(self.timestamp.timestamp())}")
