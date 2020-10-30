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
        self._symbol = symbol
        self._bid = bid
        self._ask = ask
        self._bid_size = bid_size
        self._ask_size = ask_size
        self._timestamp = timestamp

    def __eq__(self, QuoteTick other) -> bool:
        return self._timestamp == other.timestamp

    def __ne__(self, QuoteTick other) -> bool:
        return self._timestamp != other.timestamp

    def __lt__(self, QuoteTick other) -> bool:
        return self._timestamp < other.timestamp

    def __le__(self, QuoteTick other) -> bool:
        return self._timestamp <= other.timestamp

    def __gt__(self, QuoteTick other) -> bool:
        return self._timestamp > other.timestamp

    def __ge__(self, QuoteTick other) -> bool:
        return self._timestamp >= other.timestamp

    def __hash__(self) -> int:
        return hash(self._timestamp)

    def __str__(self) -> str:
        return (f"{self._symbol},"
                f"{self._bid},"
                f"{self._ask},"
                f"{self._bid_size},"
                f"{self._ask_size},"
                f"{format_iso8601(self._timestamp)}")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @property
    def symbol(self):
        """
        The ticks symbol.

        Returns
        -------
        Symbol

        """
        return self._symbol

    @property
    def bid(self):
        """
        The ticks best quoted bid price.

        Returns
        -------
        Price

        """
        return self._bid

    @property
    def ask(self):
        """
        The ticks best quoted ask price.

        Returns
        -------
        Price

        """
        return self._ask

    @property
    def bid_size(self):
        """
        The ticks quoted bid size.

        Returns
        -------
        Quantity

        """
        return self._bid_size

    @property
    def ask_size(self):
        """
        The ticks quoted ask size.

        Returns
        -------
        Quantity

        """
        return self._ask_size

    @property
    def timestamp(self):
        """
        The ticks timestamp.

        Returns
        -------
        datetime

        """
        return self._timestamp

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
            return Price((self._bid + self._ask) / 2)
        elif price_type == PriceType.BID:
            return self._bid
        elif price_type == PriceType.ASK:
            return self._ask
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
            return Quantity(self._bid_size + self._ask_size)
        elif price_type == PriceType.BID:
            return self._bid_size
        elif price_type == PriceType.ASK:
            return self._ask_size
        else:
            raise ValueError(f"Cannot extract with PriceType {price_type_to_string(price_type)}")

    @staticmethod
    cdef QuoteTick from_serializable_string_c(Symbol symbol, str values):
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
        QuoteTick

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
            Price(pieces[0]),
            Price(pieces[1]),
            Quantity(pieces[2]),
            Quantity(pieces[3]),
            datetime.fromtimestamp(long(pieces[4]) / 1000, pytz.utc),
        )

    @staticmethod
    def from_serializable_string(Symbol symbol, str values):
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
        return QuoteTick.from_serializable_string_c(symbol, values)

    cpdef str to_serializable_string(self):
        """
        Return the serializable string representation of this object.

        Returns
        -------
        str

        """
        return f"{self._bid},{self._ask},{self._bid_size},{self._ask_size},{long(self._timestamp.timestamp())}"


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

        self._symbol = symbol
        self._price = price
        self._size = size
        self._maker = maker
        self._match_id = match_id
        self._timestamp = timestamp

    def __eq__(self, TradeTick other) -> bool:
        return self._timestamp == other.timestamp

    def __ne__(self, TradeTick other) -> bool:
        return self._timestamp != other.timestamp

    def __lt__(self, TradeTick other) -> bool:
        return self._timestamp < other.timestamp

    def __le__(self, TradeTick other) -> bool:
        return self._timestamp <= other.timestamp

    def __gt__(self, TradeTick other) -> bool:
        return self._timestamp > other.timestamp

    def __ge__(self, TradeTick other) -> bool:
        return self._timestamp >= other.timestamp

    def __hash__(self) -> int:
        return hash(self._timestamp)

    def __str__(self) -> str:
        return (f"{self._symbol},"
                f"{self._price},"
                f"{self._size},"
                f"{maker_to_string(self._maker)},"
                f"{self._match_id},"
                f"{format_iso8601(self._timestamp)}")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @property
    def symbol(self):
        """
        The ticks symbol.

        Returns
        -------
        Symbol

        """
        return self._symbol

    @property
    def price(self):
        """
        The ticks traded price.

        Returns
        -------
        Price

        """
        return self._price

    @property
    def size(self):
        """
        The ticks traded size.

        Returns
        -------

        """
        return self._size

    @property
    def maker(self):
        """
        The ticks trade maker side.

        Returns
        -------
        Maker
            BUYER or SELLER.

        """
        return self._maker

    @property
    def match_id(self):
        """
        The ticks trade match identifier.

        Returns
        -------
        TradeMatchId

        """
        return self._match_id

    @property
    def timestamp(self):
        """
        The ticks timestamp.

        Returns
        -------
        datetime

        """
        return self._timestamp

    @staticmethod
    cdef TradeTick from_serializable_string_c(Symbol symbol, str values):
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
        TradeTick

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
            Price(pieces[0]),
            Quantity(pieces[1]),
            maker_from_string(pieces[2]),
            TradeMatchId(pieces[3]),
            datetime.fromtimestamp(long(pieces[4]) / 1000, pytz.utc),
        )

    @staticmethod
    def from_serializable_string(Symbol symbol, str values):
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
        TradeTick

        Raises
        ------
        ValueError
            If values is not a valid string.

        """
        return TradeTick.from_serializable_string_c(symbol, values)

    cpdef str to_serializable_string(self):
        """
        Return the serializable string representation of this object.

        Returns
        -------
        str

        """
        return (f"{self._price},"
                f"{self._size},"
                f"{maker_to_string(self._maker)},"
                f"{self._match_id},"
                f"{long(self._timestamp.timestamp())}")
