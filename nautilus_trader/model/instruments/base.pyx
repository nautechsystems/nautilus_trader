# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal
from typing import Optional

import msgspec

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport AssetClass
from nautilus_trader.core.rust.model cimport AssetType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.enums_c cimport asset_class_from_str
from nautilus_trader.model.enums_c cimport asset_class_to_str
from nautilus_trader.model.enums_c cimport asset_type_from_str
from nautilus_trader.model.enums_c cimport asset_type_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick_scheme.base cimport TICK_SCHEMES
from nautilus_trader.model.tick_scheme.base cimport get_tick_scheme


cdef class Instrument(Data):
    """
    The base class for all instruments.

    Represents a tradable financial market instrument. This class can be used to
    define an instrument, or act as a parent class for more specific instruments.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the instrument.
    native_symbol : Symbol
        The native/local symbol on the exchange for the instrument.
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
    size_increment : Price
        The minimum size increment.
    multiplier : Decimal
        The contract value multiplier (determines tick value).
    lot_size : Quantity, optional
        The rounded lot unit size (standard/board).
    margin_init : Decimal
        The initial (order) margin requirement in percentage of order value.
    margin_maint : Decimal
        The maintenance (position) margin in percentage of position value.
    maker_fee : Decimal
        The fee rate for liquidity makers as a percentage of order value.
    taker_fee : Decimal
        The fee rate for liquidity takers as a percentage of order value.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.
    price_increment : Price, optional
        The minimum price increment (tick size).
    max_quantity : Quantity, optional
        The maximum allowable order quantity.
    min_quantity : Quantity, optional
        The minimum allowable order quantity.
    max_notional : Money, optional
        The maximum allowable order notional value.
    min_notional : Money, optional
        The minimum allowable order notional value.
    max_price : Price, optional
        The maximum allowable quoted price.
    min_price : Price, optional
        The minimum allowable quoted price.
    tick_scheme_name : str, optional
        The name of the tick scheme.
    info : dict[str, object], optional
        The additional instrument information.

    Raises
    ------
    ValueError
        If `tick_scheme_name` is not a valid string.
    ValueError
        If `price_precision` is negative (< 0).
    ValueError
        If `size_precision` is negative (< 0).
    ValueError
        If `price_increment` is not positive (> 0).
    ValueError
        If `size_increment` is not positive (> 0).
    ValueError
        If `price_precision` is not equal to price_increment.precision.
    ValueError
        If `size_increment` is not equal to size_increment.precision.
    ValueError
        If `multiplier` is not positive (> 0).
    ValueError
        If `lot size` is not positive (> 0).
    ValueError
        If `max_quantity` is not positive (> 0).
    ValueError
        If `min_quantity` is negative (< 0).
    ValueError
        If `max_notional` is not positive (> 0).
    ValueError
        If `min_notional` is negative (< 0).
    ValueError
        If `max_price` is not positive (> 0).
    ValueError
        If `min_price` is negative (< 0).
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Symbol native_symbol not None,
        AssetClass asset_class,
        AssetType asset_type,
        Currency quote_currency not None,
        bint is_inverse,
        int price_precision,
        int size_precision,
        Quantity size_increment not None,
        Quantity multiplier not None,
        margin_init not None: Decimal,
        margin_maint not None: Decimal,
        maker_fee not None: Decimal,
        taker_fee not None: Decimal,
        uint64_t ts_event,
        uint64_t ts_init,
        Price price_increment: Optional[Price] = None,
        Quantity lot_size: Optional[Quantity] = None,
        Quantity max_quantity: Optional[Quantity] = None,
        Quantity min_quantity: Optional[Quantity] = None,
        Money max_notional: Optional[Money] = None,
        Money min_notional: Optional[Money] = None,
        Price max_price: Optional[Price] = None,
        Price min_price: Optional[Price] = None,
        str tick_scheme_name = None,
        dict info = None,
    ):
        Condition.not_negative_int(price_precision, "price_precision")
        Condition.not_negative_int(size_precision, "size_precision")
        Condition.positive(size_increment, "size_increment")
        Condition.equal(size_precision, size_increment.precision, "size_precision", "size_increment.precision")  # noqa
        Condition.positive(multiplier, "multiplier")

        if tick_scheme_name is not None:
            Condition.valid_string(tick_scheme_name, "tick_scheme_name")
            Condition.is_in(tick_scheme_name, TICK_SCHEMES, "tick_scheme_name", "TICK_SCHEMES")
        if price_increment is not None:
            Condition.positive(price_increment, "price_increment")
        if price_precision is not None and price_increment is not None:
            Condition.equal(price_precision, price_increment.precision, "price_precision", "price_increment.precision")  # noqa
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

        super().__init__(ts_event, ts_init)

        self.id = instrument_id
        self.native_symbol = native_symbol
        self.asset_class = asset_class
        self.asset_type = asset_type
        self.quote_currency = quote_currency
        self.is_inverse = is_inverse
        self.price_precision = price_precision
        self.price_increment = price_increment
        self.tick_scheme_name = tick_scheme_name
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

        # Assign tick scheme if named
        if self.tick_scheme_name is not None:
            self._tick_scheme = get_tick_scheme(self.tick_scheme_name)

    def __eq__(self, Instrument other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __repr__(self) -> str:  # TODO(cs): tick_scheme_name pending
        return (
            f"{type(self).__name__}"
            f"(id={self.id.to_str()}, "
            f"native_symbol={self.native_symbol}, "
            f"asset_class={asset_class_to_str(self.asset_class)}, "
            f"asset_type={asset_type_to_str(self.asset_type)}, "
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
            f"info={self.info})"
        )

    @staticmethod
    cdef Instrument base_from_dict_c(dict values):
        cdef str lot_s = values["lot_size"]
        cdef str max_q = values["max_quantity"]
        cdef str min_q = values["min_quantity"]
        cdef str max_n = values["max_notional"]
        cdef str min_n = values["min_notional"]
        cdef str max_p = values["max_price"]
        cdef str min_p = values["min_price"]
        cdef bytes info = values["info"]
        return Instrument(
            instrument_id=InstrumentId.from_str_c(values["id"]),
            native_symbol=Symbol(values["native_symbol"]),
            asset_class=asset_class_from_str(values["asset_class"]),
            asset_type=asset_type_from_str(values["asset_type"]),
            quote_currency=Currency.from_str_c(values["quote_currency"]),
            is_inverse=values["is_inverse"],
            price_precision=values["price_precision"],
            size_precision=values["size_precision"],
            price_increment=Price.from_str_c(values["price_increment"]),
            size_increment=Quantity.from_str_c(values["size_increment"]),
            multiplier=Quantity.from_str_c(values["multiplier"]),
            lot_size=Quantity.from_str_c(lot_s) if lot_s is not None else None,
            max_quantity=Quantity.from_str_c(max_q) if max_q is not None else None,
            min_quantity=Quantity.from_str_c(min_q) if min_q is not None else None,
            max_notional=Money.from_str_c(max_n) if max_n is not None else None,
            min_notional=Money.from_str_c(min_n) if min_n is not None else None,
            max_price=Price.from_str_c(max_p) if max_p is not None else None,
            min_price=Price.from_str_c(min_p) if min_p is not None else None,
            margin_init=Decimal(values["margin_init"]),
            margin_maint=Decimal(values["margin_maint"]),
            maker_fee=Decimal(values["maker_fee"]),
            taker_fee=Decimal(values["taker_fee"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            info=msgspec.json.decode(info) if info is not None else None,
        )

    @staticmethod
    cdef dict base_to_dict_c(Instrument obj):
        return {
            "type": "Instrument",
            "id": obj.id.to_str(),
            "native_symbol": obj.native_symbol.to_str(),
            "asset_class": asset_class_to_str(obj.asset_class),
            "asset_type": asset_type_to_str(obj.asset_type),
            "quote_currency": obj.quote_currency.code,
            "is_inverse": obj.is_inverse,
            "price_precision": obj.price_precision,
            "price_increment": str(obj.price_increment),
            "size_precision": obj.size_precision,
            "size_increment": str(obj.size_increment),
            "multiplier": str(obj.multiplier),
            "lot_size": str(obj.lot_size) if obj.lot_size is not None else None,
            "max_quantity": str(obj.max_quantity) if obj.max_quantity is not None else None,
            "min_quantity": str(obj.min_quantity) if obj.min_quantity is not None else None,
            "max_notional": obj.max_notional.to_str() if obj.max_notional is not None else None,
            "min_notional": obj.min_notional.to_str() if obj.min_notional is not None else None,
            "max_price": str(obj.max_price) if obj.max_price is not None else None,
            "min_price": str(obj.min_price) if obj.min_price is not None else None,
            "margin_init": str(obj.margin_init),
            "margin_maint": str(obj.margin_maint),
            "maker_fee": str(obj.maker_fee),
            "taker_fee": str(obj.taker_fee),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "info": msgspec.json.encode(obj.info) if obj.info is not None else None,
        }

    @staticmethod
    def base_from_dict(dict values) -> Instrument:
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
        return Instrument.base_from_dict_c(values)

    @staticmethod
    def base_to_dict(Instrument obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return Instrument.base_to_dict_c(obj)

    @property
    def symbol(self):
        """
        Return the instruments ticker symbol.

        Returns
        -------
        Symbol

        """
        return self.id.symbol

    @property
    def venue(self):
        """
        Return the instruments trading venue.

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
        Currency or ``None``

        """
        return None

    cpdef Currency get_settlement_currency(self):
        """
        Return the currency used to settle a trade of the instrument.

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

    cpdef Price next_bid_price(self, double value, int num_ticks=0):
        """
        Return the price `n` bid ticks away from value.

        If a given price is between two ticks, n=0 will find the nearest bid tick.

        Parameters
        ----------
        value : double
            The reference value.
        num_ticks : int, default 0
            The number of ticks to move.

        Returns
        -------
        Price

        Raises
        ------
        ValueError
            If tick scheme is not registered.

        """
        Condition.not_none(self._tick_scheme, "self._tick_scheme")

        return self._tick_scheme.next_bid_price(value=value, n=num_ticks)

    cpdef Price next_ask_price(self, double value, int num_ticks=0):
        """
        Return the price `n` ask ticks away from value.

        If a given price is between two ticks, n=0 will find the nearest ask tick.

        Parameters
        ----------
        value : double
            The reference value.
        num_ticks : int, default 0
            The number of ticks to move.

        Returns
        -------
        Price

        Raises
        ------
        ValueError
            If tick scheme is not registered.

        """
        Condition.not_none(self._tick_scheme, "self._tick_scheme")

        return self._tick_scheme.next_ask_price(value=value, n=num_ticks)

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
        Price price,
        bint inverse_as_quote=False,
    ):
        """
        Calculate the notional value.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        quantity : Quantity
            The total quantity.
        price : Price
            The price for the calculation.
        inverse_as_quote : bool
            If inverse instrument calculations use quote currency (instead of base).

        Returns
        -------
        Money

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(price, "price")

        if self.is_inverse:
            if inverse_as_quote:
                # Quantity is notional
                return Money(quantity, self.quote_currency)
            return Money(quantity.as_f64_c() * float(self.multiplier) * (1 / price.as_f64_c()), self.base_currency)
        else:
            return Money(quantity.as_f64_c() * float(self.multiplier) * price.as_f64_c(), self.quote_currency)
