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
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySideParser
from nautilus_trader.model.c_enums.position_side cimport PositionSide
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
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
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
        ts_event_ns : int64
            The UNIX timestamp (nanos) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanos) when received by the Nautilus system.
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
        super().__init__(ts_event_ns, ts_recv_ns)

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

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "type": type(self).__name__,
            "id": self.id.value,
            "asset_class": AssetClassParser.to_str(self.asset_class),
            "asset_type": AssetTypeParser.to_str(self.asset_type),
            "quote_currency": self.quote_currency.code,
            "is_inverse": self.is_inverse,
            "price_precision": self.price_precision,
            "price_increment": str(self.price_increment),
            "size_precision": self.size_precision,
            "size_increment": str(self.size_increment),
            "multiplier": str(self.multiplier),
            "lot_size": str(self.lot_size),
            "margin_init": str(self.margin_init),
            "margin_maint": str(self.margin_maint),
            "maker_fee": str(self.maker_fee),
            "taker_fee": str(self.taker_fee),
            "ts_event_ns": self.ts_event_ns,
            "ts_recv_ns": self.ts_recv_ns,
            "info": self.info,
        }

    @staticmethod
    def from_dict(dict values) -> Instrument:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        Instrument

        """
        return Instrument(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            asset_class=AssetClassParser.from_str(values["asset_class"]),
            asset_type=AssetTypeParser.from_str(values["asset_type"]),
            quote_currency=Currency.from_str_c(values["quote_currency"]),
            is_inverse=values["is_inverse"],
            price_precision=values["price_precision"],
            size_precision=values["size_precision"],
            price_increment=Price.from_str_c(values["price_increment"]),
            size_increment=Quantity.from_str_c(values["size_increment"]),
            multiplier=Quantity.from_str_c(values["multiplier"]),
            lot_size=Quantity.from_str_c(values["lot_size"]),
            margin_init=Decimal(values["margin_init"]),
            margin_maint=Decimal(values["margin_maint"]),
            maker_fee=Decimal(values["maker_fee"]),
            taker_fee=Decimal(values["taker_fee"]),
            ts_event_ns=values["ts_event_ns"],
            ts_recv_ns=values["ts_recv_ns"],
            info=values["info"],
        )

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

    cpdef Currency get_base_currency(self):
        """
        Return the instruments base currency (if applicable).

        Returns
        -------
        Currency or None

        """
        return None

    cpdef Currency get_cost_currency(self):
        """
        Return the currency used for cost and PnL calculations.

        - Standard linear instruments = quote_currency
        - Inverse instruments = base_currency
        - Quanto instruments = settlement_currency

        Returns
        -------
        Currency

        """
        if self.is_inverse:
            return self.base_currency
        else:
            return self.quote_currency

    cpdef Price make_price(self, value):
        """
        Return a new price from the given value using the instruments price
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
        Return a new quantity from the given value using the instruments size
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

    cpdef Money notional_value(
        self,
        Quantity quantity,
        close_price: Decimal,
        bint inverse_as_quote=False,
    ):
        """
        Calculate the notional value from the given parameters.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        quantity : Quantity
            The total quantity.
        close_price : Decimal or Price
            The closing price.
        inverse_as_quote : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        """
        Condition.not_none(quantity, "quantity")
        Condition.type(close_price, (Decimal, Price), "close_price")

        if self.is_inverse:
            if inverse_as_quote:
                # Quantity is notional
                return Money(quantity, self.quote_currency)
            notional_value: Decimal = quantity * self.multiplier * (1 / close_price)
            return Money(notional_value, self.base_currency)
        else:
            notional_value: Decimal = quantity * self.multiplier * close_price
            return Money(notional_value, self.quote_currency)

    cpdef Money calculate_initial_margin(
        self,
        Quantity quantity,
        Price price,
        leverage: Decimal=Decimal(1),
        bint inverse_as_quote=False,
    ):
        """
        Calculate the initial margin from the given parameters.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        quantity : Quantity
            The order quantity.
        price : Price
            The order price.
        leverage : Decimal, optional
            The current account leverage for the instrument.
        inverse_as_quote : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(price, "price")

        notional: Decimal = self.notional_value(
            quantity=quantity,
            close_price=price.as_decimal(),
            inverse_as_quote=inverse_as_quote,
        ).as_decimal()

        adjusted_notional: Decimal = notional / leverage

        margin: Decimal = adjusted_notional * self.margin_init
        margin += (adjusted_notional * self.taker_fee * 2)

        if self.is_inverse and not inverse_as_quote:
            return Money(margin, self.base_currency)
        else:
            return Money(margin, self.quote_currency)

    cpdef Money calculate_maint_margin(
        self,
        PositionSide side,
        Quantity quantity,
        Price last,
        leverage: Decimal=Decimal(1),
        bint inverse_as_quote=False,
    ):
        """
        Calculate the maintenance margin from the given parameters.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        side : PositionSide
            The currency position side.
        quantity : Quantity
            The currency position quantity.
        last : Price
            The position instruments last price.
        leverage : Decimal, optional
            The current account leverage for the instrument.
        inverse_as_quote : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        """
        # side checked in _get_close_price
        Condition.not_none(quantity, "quantity")
        Condition.not_none(last, "last")

        notional: Decimal = self.notional_value(
            quantity=quantity,
            close_price=last.as_decimal(),
            inverse_as_quote=inverse_as_quote
        ).as_decimal()

        adjusted_notional: Decimal = notional / leverage

        margin: Decimal = adjusted_notional * self.margin_maint
        margin += adjusted_notional * self.taker_fee

        if self.is_inverse and not inverse_as_quote:
            return Money(margin, self.base_currency)
        else:
            return Money(margin, self.quote_currency)

    cpdef Money calculate_commission(
        self,
        Quantity last_qty,
        last_px: Decimal,
        LiquiditySide liquidity_side,
        bint inverse_as_quote=False,
    ):
        """
        Calculate the commission generated from a transaction with the given
        parameters.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        last_qty : Quantity
            The transaction quantity.
        last_px : Decimal or Price
            The transaction price.
        liquidity_side : LiquiditySide
            The liquidity side for the transaction.
        inverse_as_quote : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        Raises
        ------
        ValueError
            If liquidity_side is NONE.

        """
        Condition.not_none(last_qty, "last_qty")
        Condition.type(last_px, (Decimal, Price), "last_px")
        Condition.not_equal(liquidity_side, LiquiditySide.NONE, "liquidity_side", "NONE")

        notional: Decimal = self.notional_value(
            quantity=last_qty,
            close_price=last_px,
            inverse_as_quote=inverse_as_quote,
        ).as_decimal()

        if liquidity_side == LiquiditySide.MAKER:
            commission: Decimal = notional * self.maker_fee
        elif liquidity_side == LiquiditySide.TAKER:
            commission: Decimal = notional * self.taker_fee
        else:
            raise RuntimeError(
                f"invalid LiquiditySide, was {LiquiditySideParser.to_str(liquidity_side)}"
            )

        if self.is_inverse and not inverse_as_quote:
            return Money(commission, self.base_currency)
        else:
            return Money(commission, self.quote_currency)
