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
            AssetClass asset_class,
            AssetType asset_type,
            Currency base_currency not None,
            Currency quote_currency not None,
            Currency settlement_currency not None,
            int price_precision,
            int size_precision,
            Decimal multiplier not None,
            Decimal tick_size not None,
            Decimal leverage not None,
            Quantity lot_size not None,
            Quantity max_quantity,  # Can be None
            Quantity min_quantity,  # Can be None
            Money max_notional,     # Can be None
            Money min_notional,     # Can be None
            Price max_price,        # Can be None
            Price min_price,        # Can be None
            Decimal margin_initial not None,
            Decimal margin_maintenance not None,
            Decimal maker_fee not None,
            Decimal taker_fee not None,
            Decimal settlement_fee not None,
            Decimal funding_rate_long not None,
            Decimal funding_rate_short not None,
            datetime timestamp not None,
    ):
        """
        Initialize a new instance of the Instrument class.

        Parameters
        ----------
        symbol : Symbol
            The symbol.
        asset_type : AssetClass
            The asset class.
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
        leverage : Decimal
            The current leverage for the instrument.
        multiplier : Decimal
            The contract value multiplier.
        lot_size : Quantity
            The rounded lot unit size.
        max_quantity : Quantity
            The maximum possible order quantity.
        min_quantity : Quantity
            The minimum possible order quantity.
        max_notional : Money
            The maximum possible order notional value.
        min_notional : Money
            The minimum possible order notional value.
        max_price : Price
            The maximum possible printed price.
        min_price : Price
            The minimum possible printed price.
        margin_initial : Decimal
            The initial margin requirement in percentage of order value.
        margin_maintenance : Decimal
            The maintenance margin in percentage of position value.
        maker_fee : Decimal
            The fee rate for liquidity makers as a percentage of order value.
        taker_fee : Decimal
            The fee rate for liquidity takers as a percentage of order value.
        settlement_fee : Decimal
            The fee rate for settlements as a percentage of order value.
        funding_rate_long : Decimal
            The funding rate for long positions.
        funding_rate_short : Decimal
            The funding rate for short positions.
        timestamp : datetime
            The timestamp the instrument was created/updated at.

        Raises
        ------
        ValueError
            If asset type is UNDEFINED.
        ValueError
            If price precision is negative (< 0).
        ValueError
            If size precision is negative (< 0).
        ValueError
            If tick size is not positive (> 0).
        ValueError
            If lot size is not positive (> 0).
        ValueError
            If leverage is not positive (> 0).

        """
        Condition.not_equal(asset_type, AssetType.UNDEFINED, 'asset_type', 'UNDEFINED')
        Condition.not_negative_int(price_precision, 'price_precision')
        Condition.not_negative_int(size_precision, 'volume_precision')
        Condition.positive(tick_size, "tick_size")
        Condition.positive(lot_size, "lot_size")
        Condition.positive(leverage, "leverage")

        # Determine standard/inverse/quanto
        cdef bint is_quanto = base_currency != quote_currency and base_currency != settlement_currency
        cdef bint is_inverse = not is_quanto and quote_currency == settlement_currency

        self.symbol = symbol
        self.asset_class = asset_class
        self.asset_type = asset_type
        self.base_currency = base_currency
        self.quote_currency = quote_currency
        self.settlement_currency = settlement_currency
        self.symbol_base_quote = f"{base_currency}/{quote_currency}"
        self.is_quanto = is_quanto
        self.is_inverse = is_inverse
        self.price_precision = price_precision
        self.size_precision = size_precision
        self.cost_precision = self.settlement_currency.precision
        self.tick_size = tick_size
        self.multiplier = multiplier
        self.leverage = leverage
        self.lot_size = lot_size
        self.max_quantity = max_quantity
        self.min_quantity = min_quantity
        self.max_notional = max_notional
        self.min_notional = min_notional
        self.max_price = max_price
        self.min_price = min_price
        self.margin_initial = margin_initial
        self.margin_maintenance = margin_maintenance
        self.maker_fee = maker_fee
        self.taker_fee = taker_fee
        self.settlement_fee = settlement_fee
        self.funding_rate_long = funding_rate_long
        self.funding_rate_short = funding_rate_short
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
        return self.symbol == other.symbol

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
        return hash(self.symbol.value)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.__class__.__name__}({self.symbol.value})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{str(self)} object at {id(self)}>"
