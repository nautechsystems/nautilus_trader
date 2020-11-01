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
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport TradeMatchId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class QuoteTick:
    """
    Represents a single quote tick in a financial market.
    """

    def __init__(
            self,
            Symbol symbol not None,
            Price bid not None,
            Price ask not None,
            Quantity bid_size not None,
            Quantity ask_size not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the `QuoteTick` class.

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
        self.symbol = symbol
        self.bid = bid
        self.ask = ask
        self.bid_size = bid_size
        self.ask_size = ask_size
        self.timestamp = timestamp

    def __eq__(self, QuoteTick other) -> bool:
        return self.timestamp == other.timestamp

    def __ne__(self, QuoteTick other) -> bool:
        return self.timestamp != other.timestamp

    def __lt__(self, QuoteTick other) -> bool:
        return self.timestamp < other.timestamp

    def __le__(self, QuoteTick other) -> bool:
        return self.timestamp <= other.timestamp

    def __gt__(self, QuoteTick other) -> bool:
        return self.timestamp > other.timestamp

    def __ge__(self, QuoteTick other) -> bool:
        return self.timestamp >= other.timestamp

    def __hash__(self) -> int:
        return hash(self.timestamp)

    def __str__(self) -> str:
        return (f"{self.symbol},"
                f"{self.bid},"
                f"{self.ask},"
                f"{self.bid_size},"
                f"{self.ask_size},"
                f"{format_iso8601(self.timestamp)}")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

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
            return Price((self.bid + self.ask) / 2)
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
            return Quantity(self.bid_size + self.ask_size)
        elif price_type == PriceType.BID:
            return self.bid_size
        elif price_type == PriceType.ASK:
            return self.ask_size
        else:
            raise ValueError(f"Cannot extract with PriceType {price_type_to_string(price_type)}")

    @staticmethod
    cdef QuoteTick from_serializable_string_c(Symbol symbol, str values):
        """
        Parse a tick from the given symbol and values string.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol.
        values : str
            The tick values string.

        Returns
        -------
        QuoteTick

        Raises
        ------
        ValueError
            If values is not a valid string.

        """
        Condition.not_none(symbol, 'symbol')
        Condition.valid_string(values, 'values')

        cdef list pieces = values.split(',', maxsplit=4)

        # noinspection: long, fromtimestamp
        # noinspection PyUnresolvedReferences
        return QuoteTick(
            symbol,
            Price(pieces[0]),
            Price(pieces[1]),
            Quantity(pieces[2]),
            Quantity(pieces[3]),
            datetime.fromtimestamp(long(pieces[4]) / 1000, pytz.utc),
        )

    @staticmethod
    def from_serializable_string(Symbol symbol, str values):
        """
        Parse a tick from the given symbol and values string.

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
        return QuoteTick.from_serializable_string_c(symbol, values)

    # noinspection: long, timestamp.timestamp()
    # noinspection PyUnresolvedReferences
    cpdef str to_serializable_string(self):
        """
        The serializable string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.bid},{self.ask},{self.bid_size},{self.ask_size},{long(self.timestamp.timestamp())}"


cdef class TradeTick:
    """
    Represents a single trade tick in a financial market.
    """

    def __init__(
            self,
            Symbol symbol not None,
            Price price not None,
            Quantity size not None,
            Maker maker,
            TradeMatchId match_id not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the `TradeTick` class.

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
        match_id : TradeMatchId
            The trade match identifier.
        timestamp : datetime
            The tick timestamp (UTC).

        Raises
        ------
        ValueError
            If maker is UNDEFINED.

        """
        Condition.not_equal(maker, Maker.UNDEFINED, "maker", "UNDEFINED")

        self.symbol = symbol
        self.price = price
        self.size = size
        self.maker = maker
        self.match_id = match_id
        self.timestamp = timestamp

    def __eq__(self, TradeTick other) -> bool:
        return self.timestamp == other.timestamp

    def __ne__(self, TradeTick other) -> bool:
        return self.timestamp != other.timestamp

    def __lt__(self, TradeTick other) -> bool:
        return self.timestamp < other.timestamp

    def __le__(self, TradeTick other) -> bool:
        return self.timestamp <= other.timestamp

    def __gt__(self, TradeTick other) -> bool:
        return self.timestamp > other.timestamp

    def __ge__(self, TradeTick other) -> bool:
        return self.timestamp >= other.timestamp

    def __hash__(self) -> int:
        return hash(self.timestamp)

    def __str__(self) -> str:
        return (f"{self.symbol},"
                f"{self.price},"
                f"{self.size},"
                f"{maker_to_string(self.maker)},"
                f"{self.match_id},"
                f"{format_iso8601(self.timestamp)}")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @staticmethod
    cdef TradeTick from_serializable_string_c(Symbol symbol, str values):
        """
        Parse a tick from the given symbol and values string.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol.
        values : str
            The tick values string.

        Returns
        -------
        TradeTick

        Raises
        ------
        ValueError
            If values is not a valid string.

        """
        Condition.not_none(symbol, 'symbol')
        Condition.valid_string(values, 'values')

        cdef list pieces = values.split(',', maxsplit=4)

        # noinspection: long, fromtimestamp
        # noinspection PyUnresolvedReferences
        return TradeTick(
            symbol,
            Price(pieces[0]),
            Quantity(pieces[1]),
            maker_from_string(pieces[2]),
            TradeMatchId(pieces[3]),
            datetime.fromtimestamp(long(pieces[4]) / 1000, pytz.utc),
        )

    @staticmethod
    def from_serializable_string(Symbol symbol, str values):
        """
        Parse a tick from the given symbol and values string.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol.
        values : str
            The tick values string.

        Returns
        -------
        TradeTick

        Raises
        ------
        ValueError
            If values is not a valid string.

        """
        return TradeTick.from_serializable_string_c(symbol, values)

    cpdef str to_serializable_string(self):
        """
        The serializable string representation of this object.

        Returns
        -------
        str

        """
        # noinspection: long, timestamp.timestamp()
        # noinspection PyUnresolvedReferences
        return (f"{self.price},"
                f"{self.size},"
                f"{maker_to_string(self.maker)},"
                f"{self.match_id},"
                f"{long(self.timestamp.timestamp())}")
