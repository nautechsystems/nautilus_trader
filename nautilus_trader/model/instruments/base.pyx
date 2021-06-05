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

from libc.stdint cimport int64_t

from decimal import Decimal

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_class cimport AssetClassParser
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.asset_type cimport AssetTypeParser
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Quantity


cdef class Instrument(Data):
    """
    The base class for all instruments.

    Represents a tradeable financial market instrument or trading pair.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        AssetClass asset_class,
        AssetType asset_type,
        Currency quote_currency not None,
        bint is_inverse,
        int price_precision,
        int size_precision,
        Price price_increment not None,
        Quantity size_increment not None,
        Quantity multiplier not None,
        Quantity lot_size,      # Can be None
        Quantity max_quantity,  # Can be None
        Quantity min_quantity,  # Can be None
        Money max_notional,     # Can be None
        Money min_notional,     # Can be None
        Price max_price,        # Can be None
        Price min_price,        # Can be None
        margin_init not None: Decimal,
        margin_maint not None: Decimal,
        maker_fee not None: Decimal,
        taker_fee not None: Decimal,
        int64_t timestamp_origin_ns,
        int64_t timestamp_ns,
        dict info=None,
    ):
        """
        Initialize a new instance of the ``Instrument`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the instrument.
        asset_class : AssetClass
            The instrument asset class.
        asset_type : AssetType
            The instrument asset type.
        quote_currency : Currency
            The quote currency.
        is_inverse : Currency
            If the instrument costing is inverse (quantity expressed in quote currency units).
        price_precision : int
            The price decimal precision.
        size_precision : int
            The trading size decimal precision.
        price_increment : Price
            The minimum price increment (tick size).
        size_increment : Price
            The minimum size increment.
        multiplier : Decimal
            The contract value multiplier (determines tick value).
        lot_size : Quantity
            The rounded lot unit size (standard/board).
        max_quantity : Quantity
            The maximum allowable order quantity.
        min_quantity : Quantity
            The minimum allowable order quantity.
        max_notional : Money
            The maximum allowable order notional value.
        min_notional : Money
            The minimum allowable order notional value.
        max_price : Price
            The maximum allowable printed price.
        min_price : Price
            The minimum allowable printed price.
        margin_init : Decimal
            The initial margin requirement in percentage of order value.
        margin_maint : Decimal
            The maintenance margin in percentage of position value.
        maker_fee : Decimal
            The fee rate for liquidity makers as a percentage of order value.
        taker_fee : Decimal
            The fee rate for liquidity takers as a percentage of order value.
        timestamp_origin_ns : int64
            The Unix timestamp (nanos) when originally occurred.
        timestamp_ns : int64
            The Unix timestamp (nanos) when received by the Nautilus system.
        info : dict[str, object], optional
            The additional instrument information.

        Raises
        ------
        ValueError
            If price_precision is negative (< 0).
        ValueError
            If size_precision is negative (< 0).
        ValueError
            If price_increment is not positive (> 0).
        ValueError
            If size_increment is not positive (> 0).
        ValueError
            If price_precision is not equal to price_increment.precision.
        ValueError
            If size_increment is not equal to size_increment.precision.
        ValueError
            If multiplier is not positive (> 0).
        ValueError
            If lot size is not positive (> 0).
        ValueError
            If max_quantity is not positive (> 0).
        ValueError
            If min_quantity is negative (< 0).
        ValueError
            If max_notional is not positive (> 0).
        ValueError
            If min_notional is negative (< 0).
        ValueError
            If max_price is not positive (> 0).
        ValueError
            If min_price is negative (< 0).

        """
        Condition.not_negative_int(price_precision, "price_precision")
        Condition.not_negative_int(size_precision, "size_precision")
        Condition.positive(price_increment, "price_increment")
        Condition.positive(size_increment, "size_increment")
        Condition.equal(price_precision, price_increment.precision, "price_precision", "price_increment.precision")  # noqa
        Condition.equal(size_precision, size_increment.precision, "size_precision", "size_increment.precision")  # noqa
        Condition.positive(multiplier, "multiplier")
        if lot_size is not None:
            Condition.positive(lot_size, "lot_size")
        if max_quantity is not None:
            Condition.positive(max_quantity, "max_quantity")
        if min_quantity is not None:
            Condition.not_negative(min_quantity, "min_quantity")
        if max_notional is not None:
            Condition.positive(max_notional, "max_notional")
        if min_notional is not None:
            Condition.not_negative(min_notional, "min_notional")
        if max_price is not None:
            Condition.positive(max_price, "max_price")
        if min_price is not None:
            Condition.not_negative(min_price, "min_price")
        Condition.type(margin_init, Decimal, "margin_init")
        Condition.not_negative(margin_init, "margin_init")
        Condition.type(margin_maint, Decimal, "margin_maint")
        Condition.not_negative(margin_maint, "margin_maint")
        Condition.type(maker_fee, Decimal, "maker_fee")
        Condition.type(taker_fee, Decimal, "taker_fee")
        super().__init__(timestamp_origin_ns, timestamp_ns)

        self.id = instrument_id
        self.asset_class = asset_class
        self.asset_type = asset_type
        self.quote_currency = quote_currency
        self.is_inverse = is_inverse
        self.price_precision = price_precision
        self.price_increment = price_increment
        self.size_precision = size_precision
        self.size_increment = size_increment
        self.multiplier = multiplier
        self.lot_size = lot_size
        self.max_quantity = max_quantity
        self.min_quantity = min_quantity
        self.max_notional = max_notional
        self.min_notional = min_notional
        self.max_price = max_price
        self.min_price = min_price
        self.margin_init = margin_init
        self.margin_maint = margin_maint
        self.maker_fee = maker_fee
        self.taker_fee = taker_fee
        self.info = info

    def __eq__(self, Instrument other) -> bool:
        return self.id.value == other.id.value

    def __ne__(self, Instrument other) -> bool:
        return self.id.value != other.id.value

    def __hash__(self) -> int:
        return hash(self.id.value)

    def __repr__(self) -> str:
        return (f"{type(self).__name__}"
                f"(id={self.id.value}, "
                f"symbol={self.id.symbol}, "
                f"asset_class={AssetClassParser.to_str(self.asset_class)}, "
                f"asset_type={AssetTypeParser.to_str(self.asset_type)}, "
                f"quote_currency={self.quote_currency}, "
                f"is_inverse={self.is_inverse}, "
                f"price_precision={self.price_precision}, "
                f"price_increment={self.price_increment}, "
                f"size_precision={self.size_precision}, "
                f"size_increment={self.size_increment}, "
                f"multiplier={self.multiplier}, "
                f"lot_size={self.lot_size}, "
                f"margin_init={self.margin_init}, "
                f"margin_maint={self.margin_maint}, "
                f"maker_fee={self.maker_fee}, "
                f"taker_fee={self.taker_fee}, "
                f"info={self.info})")

    @property
    def symbol(self):
        """
        The instruments ticker symbol.

        Returns
        -------
        Symbol

        """
        return self.id.symbol

    @property
    def venue(self):
        """
        The instruments trading venue.

        Returns
        -------
        Venue

        """
        return self.id.venue

    cpdef Price make_price(self, value):
        """
        Create a new price from the given value using the instruments price
        precision.

        Parameters
        ----------
        value : integer, float, str or Decimal
            The value of the price.

        Returns
        -------
        Price

        """
        return Price(float(value), precision=self.price_precision)

    cpdef Quantity make_qty(self, value):
        """
        Create a new quantity from the given value using the instruments size
        precision.

        Parameters
        ----------
        value : integer, float, str or Decimal
            The value of the quantity.

        Returns
        -------
        Quantity

        """
        return Quantity(float(value), precision=self.size_precision)
