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

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Decimal
from nautilus_trader.model.objects cimport Quantity


cdef class Instrument:
    """
    Represents a tradeable financial market instrument.
    """

    def __init__(
            self,
            Symbol symbol not None,
            Currency quote_currency not None,
            SecurityType security_type,
            int price_precision,
            int size_precision,
            Decimal tick_size not None,
            Quantity lot_size not None,
            Quantity min_trade_size not None,
            Quantity max_trade_size not None,
            Decimal rollover_interest_buy not None,
            Decimal rollover_interest_sell not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the Instrument class.

        Parameters
        ----------
        symbol : Symbol
            The symbol.
        quote_currency : Currency
            The quote currency.
        security_type : SecurityType
            The security type.
        price_precision : int
            The price decimal precision.
        size_precision : int
            The trading size decimal precision.
        tick_size : Decimal
            The tick size.
        lot_size : Quantity
            The rounded lot size.
        min_trade_size : Quantity
            The minimum possible trade size.
        max_trade_size : Quantity
            The maximum possible trade size.
        rollover_interest_buy : Decimal
            The rollover interest for long positions.
        rollover_interest_sell : Decimal
            The rollover interest for short positions.
        timestamp : datetime
            The timestamp the instrument was created/updated at.

        """
        Condition.not_equal(security_type, SecurityType.UNDEFINED, 'security_type', 'UNDEFINED')
        Condition.not_negative_int(price_precision, 'price_precision')
        Condition.not_negative_int(size_precision, 'volume_precision')
        Condition.equal(size_precision, lot_size.precision, 'size_precision', 'round_lot_size.precision')
        Condition.equal(size_precision, min_trade_size.precision, 'size_precision', 'min_trade_size.precision')
        Condition.equal(size_precision, max_trade_size.precision, 'size_precision', 'max_trade_size.precision')

        self.id = InstrumentId(symbol.value)
        self.symbol = symbol
        self.quote_currency = quote_currency
        self.security_type = security_type
        self.price_precision = price_precision
        self.size_precision = size_precision
        self.tick_size = tick_size
        self.lot_size = lot_size
        self.min_trade_size = min_trade_size
        self.max_trade_size = max_trade_size
        self.rollover_interest_buy = rollover_interest_buy
        self.rollover_interest_sell = rollover_interest_sell
        self.timestamp = timestamp

    def __eq__(self, Instrument other) -> bool:
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
        return self.id == other.id

    def __ne__(self, Instrument other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """
        Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash(self.symbol.to_string())

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.__class__.__name__}({self.symbol})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{str(self)} object at {id(self)}>"


cdef class ForexInstrument(Instrument):
    """
    Represents a tradeable FOREX currency pair.
    """

    def __init__(
            self,
            Symbol symbol not None,
            int price_precision,
            int size_precision,
            int min_stop_distance_entry,
            int min_stop_distance,
            int min_limit_distance_entry,
            int min_limit_distance,
            Decimal tick_size not None,
            Quantity lot_size not None,
            Quantity min_trade_size not None,
            Quantity max_trade_size not None,
            Decimal rollover_interest_buy not None,
            Decimal rollover_interest_sell not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the ForexInstrument class.

        Parameters
        ----------
        symbol : Symbol
            The symbol.
        price_precision : int
            The price decimal precision.
        size_precision : int
            The trading size decimal precision.
        min_stop_distance_entry : int
            The minimum distance for stop entry orders.
        min_stop_distance : int
            The minimum tick distance for stop orders.
        min_limit_distance_entry : int
            The minimum distance for limit entry orders.
        min_limit_distance : int
            The minimum tick distance for limit orders.
        tick_size : Decimal
            The tick size.
        lot_size : Quantity
            The rounded lot size.
        min_trade_size : Quantity
            The minimum possible trade size.
        max_trade_size : Quantity
            The maximum possible trade size.
        rollover_interest_buy : Decimal
            The rollover interest for long positions.
        rollover_interest_sell : Decimal
            The rollover interest for short positions.
        timestamp : datetime
            The timestamp the instrument was created/updated at.

        """
        super().__init__(
            symbol,
            Currency.from_string_c(symbol.code[-3:]),
            SecurityType.FOREX,
            price_precision,
            size_precision,
            tick_size,
            lot_size,
            min_trade_size,
            max_trade_size,
            rollover_interest_buy,
            rollover_interest_sell,
            timestamp,
        )

        self.base_currency = Currency.from_string_c(symbol.code[:3])
        self.min_stop_distance_entry = min_stop_distance_entry
        self.min_stop_distance = min_stop_distance
        self.min_limit_distance_entry = min_limit_distance_entry
        self.min_limit_distance = min_limit_distance
