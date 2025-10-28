# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd
import pytz

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.rust.model cimport AssetClass
from nautilus_trader.core.rust.model cimport InstrumentClass
from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.model.functions cimport asset_class_from_str
from nautilus_trader.model.functions cimport asset_class_to_str
from nautilus_trader.model.functions cimport instrument_class_from_str
from nautilus_trader.model.functions cimport instrument_class_to_str
from nautilus_trader.model.functions cimport option_kind_from_str
from nautilus_trader.model.functions cimport option_kind_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.base cimport Price
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity


cdef class CryptoOption(Instrument):
    """
    Represents an option instrument with crypto assets as underlying and for settlement.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    raw_symbol : Symbol
        The raw/local/native symbol for the instrument, assigned by the venue.
    underlying : Currency
        The underlying asset.
    quote_currency : Currency
        The contract quote currency.
    settlement_currency : Currency
        The settlement currency.
    is_inverse : bool
        If the instrument costing is inverse (quantity expressed in quote currency units).
    option_kind : OptionKind
        The kind of option (PUT | CALL).
    strike_price : Price
        The option strike price.
    activation_ns : uint64_t
        UNIX timestamp (nanoseconds) for contract activation.
    expiration_ns : uint64_t
        UNIX timestamp (nanoseconds) for contract expiration.
    price_precision : int
        The price decimal precision.
    size_precision : int
        The trading size decimal precision.
    price_increment : Price
        The minimum price increment (tick size).
    size_increment : Quantity
        The minimum size increment.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.
    multiplier : Quantity, default 1
        The contract multiplier.
    lot_size : Quantity, default 1
        The rounded lot unit size (standard/board).
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
    margin_init : Decimal, optional
        The initial (order) margin requirement in percentage of order value.
    margin_maint : Decimal, optional
        The maintenance (position) margin in percentage of position value.
    maker_fee : Decimal, optional
        The fee rate for liquidity makers as a percentage of order value.
    taker_fee : Decimal, optional
        The fee rate for liquidity takers as a percentage of order value.
    tick_scheme_name : str, optional
        The name of the tick scheme.
    info : dict[str, object], optional
        The additional instrument information.

    Raises
    ------
    ValueError
        If `price_precision` is negative (< 0).
    ValueError
        If `size_precision` is negative (< 0).
    ValueError
        If `price_increment` is not positive (> 0).
    ValueError
        If `size_increment` is not positive (> 0).
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
    ValueError
        If `margin_init` is negative (< 0).
    ValueError
        If `margin_maint` is negative (< 0).

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Symbol raw_symbol not None,
        Currency underlying not None,
        Currency quote_currency not None,
        Currency settlement_currency not None,
        bint is_inverse,
        OptionKind option_kind,
        Price strike_price not None,
        uint64_t activation_ns,
        uint64_t expiration_ns,
        int price_precision,
        int size_precision,
        Price price_increment not None,
        Quantity size_increment not None,
        uint64_t ts_event,
        uint64_t ts_init,
        Quantity multiplier = Quantity.from_int_c(1),
        Quantity lot_size = Quantity.from_int_c(1),
        Quantity max_quantity: Quantity | None = None,
        Quantity min_quantity: Quantity | None = None,
        Money max_notional: Money | None = None,
        Money min_notional: Money | None = None,
        Price max_price: Price | None = None,
        Price min_price: Price | None = None,
        margin_init: Decimal | None = None,
        margin_maint: Decimal | None = None,
        maker_fee: Decimal | None = None,
        taker_fee: Decimal | None = None,
        str tick_scheme_name = None,
        dict info = None,
    ) -> None:
        super().__init__(
            instrument_id=instrument_id,
            raw_symbol=raw_symbol,
            asset_class=AssetClass.CRYPTOCURRENCY,
            instrument_class=InstrumentClass.OPTION,
            quote_currency=quote_currency,
            is_inverse=is_inverse,
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=price_increment,
            size_increment=size_increment,
            multiplier=multiplier,
            lot_size=lot_size,
            max_quantity=max_quantity,
            min_quantity=min_quantity,
            max_notional=max_notional,
            min_notional=min_notional,
            max_price=max_price,
            min_price=min_price,
            margin_init=margin_init or Decimal(0),
            margin_maint=margin_maint or Decimal(0),
            maker_fee=maker_fee or Decimal(0),
            taker_fee=taker_fee or Decimal(0),
            ts_event=ts_event,
            ts_init=ts_init,
            tick_scheme_name=tick_scheme_name,
            info=info,
        )

        self.underlying = underlying
        self.settlement_currency = settlement_currency
        self.option_kind = option_kind
        self.strike_price = strike_price
        self.activation_ns = activation_ns
        self.expiration_ns = expiration_ns

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}"
            f"(id={self.id.to_str()}, "
            f"raw_symbol={self.raw_symbol}, "
            f"asset_class={asset_class_to_str(self.asset_class)}, "
            f"instrument_class={instrument_class_to_str(self.instrument_class)}, "
            f"is_inverse={self.is_inverse}, "
            f"underlying={self.underlying}, "
            f"quote_currency={self.quote_currency}, "
            f"option_kind={option_kind_to_str(self.option_kind)}, "
            f"strike_price={self.strike_price}, "
            f"activation={format_iso8601(self.activation_utc, nanos_precision=False)}, "
            f"expiration={format_iso8601(self.expiration_utc, nanos_precision=False)}, "
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

    cpdef Currency get_base_currency(self):
        """
        Return the instruments base currency (underlying).

        Returns
        -------
        Currency

        """
        return self.underlying

    cpdef Currency get_settlement_currency(self):
        """
        Return the currency used to settle a trade of the instrument.

        Returns
        -------
        Currency

        """
        return self.settlement_currency

    cpdef Currency get_cost_currency(self):
        """
        Return the currency used for PnL calculations for the instrument.

        - Standard linear instruments = quote_currency
        - Inverse instruments = underlying (base currency)

        Returns
        -------
        Currency

        """
        if self.is_inverse:
            return self.underlying
        else:
            return self.quote_currency

    cpdef Money notional_value(
        self,
        Quantity quantity,
        Price price,
        bint use_quote_for_inverse=False,
    ):
        """
        Calculate the notional value.

        Result will be in quote currency for standard instruments, or underlying
        currency for inverse instruments.

        Parameters
        ----------
        quantity : Quantity
            The total quantity.
        price : Price
            The price for the calculation.
        use_quote_for_inverse : bool
            For inverse instruments only: if True, treats the quantity as already representing
            notional value in quote currency and returns it directly without calculation.
            This is useful when quantity already represents a USD value that doesn't need
            conversion (e.g., for display purposes). Has no effect on linear instruments.

        Returns
        -------
        Money

        """
        Condition.not_none(quantity, "quantity")
        Condition.not_none(price, "price")

        if self.is_inverse:
            if use_quote_for_inverse:
                # Quantity is notional in quote currency
                return Money(quantity, self.quote_currency)

            return Money(quantity.as_f64_c() * float(self.multiplier) * (1.0 / price.as_f64_c()), self.underlying)
        else:
            return Money(quantity.as_f64_c() * float(self.multiplier) * price.as_f64_c(), self.quote_currency)

    @property
    def activation_utc(self) -> pd.Timestamp:
        """
        Return the contract activation timestamp (UTC).

        Returns
        -------
        pd.Timestamp
            tz-aware UTC.

        """
        return pd.Timestamp(self.activation_ns, tz=pytz.utc)

    @property
    def expiration_utc(self) -> pd.Timestamp:
        """
        Return the contract expiration timestamp (UTC).

        Returns
        -------
        pd.Timestamp
            tz-aware UTC.

        """
        return pd.Timestamp(self.expiration_ns, tz=pytz.utc)

    @staticmethod
    cdef CryptoOption from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str max_q = values["max_quantity"]
        cdef str min_q = values["min_quantity"]
        cdef str max_n = values["max_notional"]
        cdef str min_n = values["min_notional"]
        cdef str max_p = values["max_price"]
        cdef str min_p = values["min_price"]
        return CryptoOption(
            instrument_id=InstrumentId.from_str_c(values["id"]),
            raw_symbol=Symbol(values["raw_symbol"]),
            underlying=Currency.from_str_c(values["underlying"]),
            quote_currency=Currency.from_str_c(values["quote_currency"]),
            settlement_currency=Currency.from_str_c(values["settlement_currency"]),
            is_inverse=values["is_inverse"],
            option_kind=option_kind_from_str(values["option_kind"]),
            strike_price=Price.from_str(values["strike_price"]),
            activation_ns=values["activation_ns"],
            expiration_ns=values["expiration_ns"],
            price_precision=values["price_precision"],
            size_precision=values["size_precision"],
            price_increment=Price.from_str_c(values["price_increment"]),
            size_increment=Quantity.from_str_c(values["size_increment"]),
            multiplier=Quantity.from_str(values["multiplier"]),
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
            tick_scheme_name=values.get("tick_scheme_name"),
            info=values.get("info"),
        )

    @staticmethod
    cdef dict to_dict_c(CryptoOption obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "CryptoOption",
            "id": obj.id.to_str(),
            "raw_symbol": obj.raw_symbol.to_str(),
            "underlying": obj.underlying.code,
            "quote_currency": obj.quote_currency.code,
            "settlement_currency": obj.settlement_currency.code,
            "is_inverse": obj.is_inverse,
            "option_kind": option_kind_to_str(obj.option_kind),
            "strike_price": str(obj.strike_price),
            "activation_ns": obj.activation_ns,
            "expiration_ns": obj.expiration_ns,
            "price_precision": obj.price_precision,
            "price_increment": str(obj.price_increment),
            "size_precision": obj.size_precision,
            "size_increment": str(obj.size_increment),
            "multiplier": str(obj.multiplier),
            "lot_size": str(obj.lot_size),
            "max_quantity": str(obj.max_quantity) if obj.max_quantity is not None else None,
            "min_quantity": str(obj.min_quantity) if obj.min_quantity is not None else None,
            "max_notional": str(obj.max_notional) if obj.max_notional is not None else None,
            "min_notional": str(obj.min_notional) if obj.min_notional is not None else None,
            "max_price": str(obj.max_price) if obj.max_price is not None else None,
            "min_price": str(obj.min_price) if obj.min_price is not None else None,
            "margin_init": str(obj.margin_init),
            "margin_maint": str(obj.margin_maint),
            "maker_fee": str(obj.maker_fee),
            "taker_fee": str(obj.taker_fee),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "tick_scheme_name": obj.tick_scheme_name,
            "info": obj.info,
        }

    @staticmethod
    cdef CryptoOption from_pyo3_c(pyo3_instrument):
        Condition.not_none(pyo3_instrument, "pyo3_instrument")
        return CryptoOption(
            instrument_id=InstrumentId.from_str_c(pyo3_instrument.id.value),
            raw_symbol=Symbol(pyo3_instrument.raw_symbol.value),
            underlying=Currency.from_str_c(pyo3_instrument.underlying.code),
            quote_currency=Currency.from_str_c(pyo3_instrument.quote_currency.code),
            settlement_currency=Currency.from_str_c(pyo3_instrument.settlement_currency.code),
            is_inverse=pyo3_instrument.is_inverse,
            option_kind=option_kind_from_str(str(pyo3_instrument.option_kind)),
            strike_price=Price.from_raw_c(pyo3_instrument.strike_price.raw, pyo3_instrument.strike_price.precision),
            activation_ns=pyo3_instrument.activation_ns,
            expiration_ns=pyo3_instrument.expiration_ns,
            price_precision=pyo3_instrument.price_precision,
            size_precision=pyo3_instrument.size_precision,
            price_increment=Price.from_raw_c(pyo3_instrument.price_increment.raw, pyo3_instrument.price_precision),
            size_increment=Quantity.from_raw_c(pyo3_instrument.size_increment.raw, pyo3_instrument.size_precision),
            multiplier=Quantity.from_raw_c(pyo3_instrument.multiplier.raw, pyo3_instrument.multiplier.precision),
            lot_size=Quantity.from_raw_c(pyo3_instrument.lot_size.raw, pyo3_instrument.lot_size.precision),
            max_quantity=Quantity.from_raw_c(pyo3_instrument.max_quantity.raw,pyo3_instrument.max_quantity.precision) if pyo3_instrument.max_quantity is not None else None,
            min_quantity=Quantity.from_raw_c(pyo3_instrument.min_quantity.raw, pyo3_instrument.min_quantity.precision) if pyo3_instrument.min_quantity is not None else None,
            max_notional=Money.from_str_c(str(pyo3_instrument.max_notional)) if pyo3_instrument.max_notional is not None else None,
            min_notional=Money.from_str_c(str(pyo3_instrument.min_notional)) if pyo3_instrument.min_notional is not None else None,
            max_price=Price.from_raw_c(pyo3_instrument.max_price.raw, pyo3_instrument.max_price.precision) if pyo3_instrument.max_price is not None else None,
            min_price=Price.from_raw_c(pyo3_instrument.min_price.raw, pyo3_instrument.min_price.precision) if pyo3_instrument.min_price is not None else None,
            margin_init=Decimal(pyo3_instrument.margin_init),
            margin_maint=Decimal(pyo3_instrument.margin_maint),
            maker_fee=Decimal(pyo3_instrument.maker_fee),
            taker_fee=Decimal(pyo3_instrument.taker_fee),
            info=pyo3_instrument.info,
            ts_event=pyo3_instrument.ts_event,
            ts_init=pyo3_instrument.ts_init,
        )

    @staticmethod
    def from_dict(dict values) -> CryptoOption:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        CryptoOption

        """
        return CryptoOption.from_dict_c(values)

    @staticmethod
    def to_dict(CryptoOption obj) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return CryptoOption.to_dict_c(obj)

    @staticmethod
    def from_pyo3(pyo3_instrument) -> CryptoOption:
        """
        Return legacy Cython option contract instrument converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_instrument : nautilus_pyo3.CryptoOption
            The pyo3 Rust option contract instrument to convert from.

        Returns
        -------
        CryptoOption

        """
        return CryptoOption.from_pyo3_c(pyo3_instrument)
