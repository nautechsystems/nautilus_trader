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
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Decimal
from nautilus_trader.model.objects cimport Quantity


cdef class Instrument:
    """
    The base class for all instruments. Represents a tradeable financial market
    instrument.
    """

    def __init__(
            self,
            Symbol symbol not None,
            AssetType asset_type,
            Currency base_currency not None,
            Currency quote_currency not None,
            Currency settlement_currency not None,
            int price_precision,
            int size_precision,
            Decimal tick_size not None,
            Quantity lot_size not None,
            object min_trade_size not None,
            object max_trade_size not None,
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
        asset_type : AssetType
            The asset type.
        base_currency : Currency
            The base currency.
        quote_currency : Currency
            The quote currency.
        settlement_currency : Currency
            The settlement currency.
        price_precision : int
            The price decimal precision.
        size_precision : int
            The trading size decimal precision.
        tick_size : Decimal
            The tick size.
        lot_size : Quantity
            The rounded lot unit size.
        min_trade_size : Quantity or Money
            The minimum possible trade size.
        max_trade_size : Quantity or Money
            The maximum possible trade size.
        rollover_interest_buy : Decimal
            The rollover interest for long positions.
        rollover_interest_sell : Decimal
            The rollover interest for short positions.
        timestamp : datetime
            The timestamp the instrument was created/updated at.

        """
        Condition.not_equal(asset_type, AssetType.UNDEFINED, 'asset_type', 'UNDEFINED')
        Condition.not_negative_int(price_precision, 'price_precision')
        Condition.not_negative_int(size_precision, 'volume_precision')

        # Determine standard/inverse/quanto
        cdef bint is_quanto = base_currency != quote_currency and base_currency != settlement_currency
        cdef bint is_inverse = not is_quanto and quote_currency == settlement_currency
        cdef bint is_standard = not is_quanto and not is_inverse

        self.id = InstrumentId(symbol.value)
        self.symbol = symbol
        self.asset_type = asset_type
        self.base_currency = base_currency
        self.quote_currency = quote_currency
        self.settlement_currency = settlement_currency
        self.is_quanto = is_quanto
        self.is_inverse = is_inverse
        self.is_standard = is_standard
        self.price_precision = price_precision
        self.size_precision = size_precision
        self.cost_precision = self.settlement_currency.precision
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
        return not self == other

    def __hash__(self) -> int:
        """
        Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash(self.id.value)

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
