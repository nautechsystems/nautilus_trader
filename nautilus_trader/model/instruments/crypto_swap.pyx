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

from libc.stdint cimport uint64_t

from decimal import Decimal

from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class CryptoSwap(Instrument):
    """
    Represents a crypto perpetual swap instrument.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Currency base_currency not None,
        Currency quote_currency not None,
        Currency settlement_currency not None,
        bint is_inverse,
        int price_precision,
        int size_precision,
        Price price_increment not None,
        Quantity size_increment not None,
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
        uint64_t ts_event_ns,
        uint64_t ts_recv_ns,
        dict info=None,
    ):
        """
        Initialize a new instance of the ``CryptoSwap` instrument class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the instrument.
        base_currency : Currency, optional
            The base currency.
        quote_currency : Currency
            The quote currency.
        quote_currency : Currency
            The settlement currency.
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
        ts_event_ns: uint64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns: uint64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.
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
        super().__init__(
            instrument_id=instrument_id,
            asset_class=AssetClass.CRYPTO,
            asset_type=AssetType.SWAP,
            quote_currency=quote_currency,
            is_inverse=is_inverse,
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=price_increment,
            size_increment=size_increment,
            multiplier=Quantity.from_int_c(1),
            lot_size=Quantity.from_int_c(1),
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
            ts_event_ns=ts_event_ns,
            ts_recv_ns=ts_recv_ns,
            info=info,
        )

        self.base_currency = base_currency
        self.settlement_currency = settlement_currency
        if settlement_currency != base_currency and settlement_currency != quote_currency:
            self.is_quanto = True
        else:
            self.is_quanto = False

    cpdef Currency get_base_currency(self):
        """
        Return the instruments base currency.

        Returns
        -------
        Currency

        """
        return self.base_currency

    @staticmethod
    cdef CryptoSwap from_dict_c(dict values):
        cdef str max_q = values["max_quantity"]
        cdef str min_q = values["min_quantity"]
        cdef str max_n = values["max_notional"]
        cdef str min_n = values["min_notional"]
        cdef str max_p = values["max_price"]
        cdef str min_p = values["min_price"]
        return CryptoSwap(
            instrument_id=InstrumentId.from_str_c(values["id"]),
            base_currency=Currency.from_str_c(values["base_currency"]),
            quote_currency=Currency.from_str_c(values["quote_currency"]),
            settlement_currency=Currency.from_str_c(values["settlement_currency"]),
            is_inverse=values["is_inverse"],
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
            ts_event_ns=values["ts_event_ns"],
            ts_recv_ns=values["ts_recv_ns"],
            info=values["info"],
        )

    @staticmethod
    def from_dict(dict values) -> CryptoSwap:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        CryptoSwap

        """
        return CryptoSwap.from_dict_c(values)

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
            "base_currency": self.base_currency.code,
            "quote_currency": self.quote_currency.code,
            "settlement_currency": self.settlement_currency.code,
            "is_inverse": self.is_inverse,
            "price_precision": self.price_precision,
            "price_increment": str(self.price_increment),
            "size_precision": self.size_precision,
            "size_increment": str(self.size_increment),
            "max_quantity": str(self.max_quantity) if self.max_quantity is not None else None,
            "min_quantity": str(self.min_quantity) if self.min_quantity is not None else None,
            "max_notional": self.max_notional.to_str() if self.max_notional is not None else None,
            "min_notional": self.min_notional.to_str() if self.min_notional is not None else None,
            "max_price": str(self.max_price) if self.max_price is not None else None,
            "min_price": str(self.min_price) if self.min_price is not None else None,
            "margin_init": str(self.margin_init),
            "margin_maint": str(self.margin_maint),
            "maker_fee": str(self.maker_fee),
            "taker_fee": str(self.taker_fee),
            "ts_event_ns": self.ts_event_ns,
            "ts_recv_ns": self.ts_recv_ns,
            "info": self.info,
        }
