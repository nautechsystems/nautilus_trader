# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport date
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class CryptoFuture(Instrument):
    """
    Represents a `Deliverable Futures Contract` instrument, with crypto assets
    as underlying and for settlement.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the instrument.
    native_symbol : Symbol
        The native/local symbol on the exchange for the instrument.
    underlying : Currency
        The underlying asset.
    quote_currency : Currency
        The contract quote currency.
    expiry_date : date
        The contract expiry date.
    price_precision : int
        The price decimal precision.
    size_precision : int
        The trading size decimal precision.
    price_increment : Price
        The minimum price increment (tick size).
    size_increment : Quantity
        The minimum size increment.
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
        If `price_precision` is not equal to price_increment.precision.
    ValueError
        If `size_increment` is not equal to size_increment.precision.
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
        Currency underlying not None,
        Currency quote_currency not None,
        Currency settlement_currency not None,
        date expiry_date,
        int price_precision,
        int size_precision,
        Price price_increment not None,
        Quantity size_increment not None,
        margin_init not None: Decimal,
        margin_maint not None: Decimal,
        maker_fee not None: Decimal,
        taker_fee not None: Decimal,
        uint64_t ts_event,
        uint64_t ts_init,
        multiplier=Quantity.from_int_c(1),
        lot_size=Quantity.from_int_c(1),
        Quantity max_quantity: Optional[Quantity] = None,
        Quantity min_quantity: Optional[Quantity] = None,
        Money max_notional: Optional[Money] = None,
        Money min_notional: Optional[Money] = None,
        Price max_price: Optional[Price] = None,
        Price min_price: Optional[Price] = None,
        dict info = None,
    ):
        super().__init__(
            instrument_id=instrument_id,
            native_symbol=native_symbol,
            asset_class=AssetClass.CRYPTO,
            asset_type=AssetType.FUTURE,
            quote_currency=quote_currency,
            is_inverse=False,
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
            margin_init=margin_init,
            margin_maint=margin_maint,
            maker_fee=maker_fee,
            taker_fee=taker_fee,
            ts_event=ts_event,
            ts_init=ts_init,
            info=info,
        )

        self.underlying = underlying
        self.settlement_currency = settlement_currency
        self.expiry_date = expiry_date

    cpdef Currency get_base_currency(self):
        """
        Return the instruments base currency.

        Returns
        -------
        Currency

        """
        return self.underlying

    @staticmethod
    cdef CryptoFuture from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str max_q = values["max_quantity"]
        cdef str min_q = values["min_quantity"]
        cdef str max_n = values["max_notional"]
        cdef str min_n = values["min_notional"]
        cdef str max_p = values["max_price"]
        cdef str min_p = values["min_price"]
        cdef bytes info = values["info"]
        return CryptoFuture(
            instrument_id=InstrumentId.from_str_c(values["id"]),
            native_symbol=Symbol(values["native_symbol"]),
            underlying=Currency.from_str_c(values["underlying"]),
            quote_currency=Currency.from_str_c(values["quote_currency"]),
            settlement_currency=Currency.from_str_c(values["settlement_currency"]),
            expiry_date=date.fromisoformat(values['expiry_date']),
            price_precision=values["price_precision"],
            size_precision=values["size_precision"],
            price_increment=Price.from_str_c(values["price_increment"]),
            size_increment=Quantity.from_str_c(values["size_increment"]),
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
    cdef dict to_dict_c(CryptoFuture obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "CryptoFuture",
            "id": obj.id.to_str(),
            "native_symbol": obj.native_symbol.to_str(),
            "underlying": obj.underlying.code,
            "quote_currency": obj.quote_currency.code,
            "settlement_currency": obj.settlement_currency.code,
            "expiry_date": obj.expiry_date.isoformat(),
            "price_precision": obj.price_precision,
            "price_increment": str(obj.price_increment),
            "size_precision": obj.size_precision,
            "size_increment": str(obj.size_increment),
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
    def from_dict(dict values) -> CryptoFuture:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        CryptoFuture

        """
        return CryptoFuture.from_dict_c(values)

    @staticmethod
    def to_dict(CryptoFuture obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return CryptoFuture.to_dict_c(obj)
