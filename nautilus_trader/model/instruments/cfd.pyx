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

import json
from libc.stdint cimport int64_t

from decimal import Decimal

from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_class cimport AssetClassParser
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class CFDInstrument(Instrument):
    """
    Represents an CFD instrument.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        AssetClass asset_class,
        Currency quote_currency not None,
        int price_precision,
        int size_precision,
        Price price_increment not None,
        Quantity size_increment not None,
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
        Initialize a new instance of the ``CFDInstrument`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the instrument.
        asset_class : AssetClass
            The instrument asset class.
        quote_currency : Currency
            The quote currency.
        price_precision : int
            The price decimal precision.
        size_precision : int
            The trading size decimal precision.
        price_increment : Price
            The minimum price increment (tick size).
        size_increment : Price
            The minimum size increment.
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
        ts_event_ns: int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns: int64
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
        super().__init__(
            instrument_id=instrument_id,
            asset_class=asset_class,
            asset_type=AssetType.CFD,
            quote_currency=quote_currency,
            is_inverse=False,
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=price_increment,
            size_increment=size_increment,
            multiplier=Quantity.from_int_c(1),
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
            ts_event_ns=ts_event_ns,
            ts_recv_ns=ts_recv_ns,
            info=info,
        )

    @staticmethod
    cdef CFDInstrument from_dict_c(dict values):
        cdef str max_q = values["max_quantity"]
        cdef str min_q = values["min_quantity"]
        cdef str max_n = values["max_notional"]
        cdef str min_n = values["min_notional"]
        cdef str max_p = values["max_price"]
        cdef str min_p = values["min_price"]
        cdef str info = values["info"]
        return CFDInstrument(
            instrument_id=InstrumentId.from_str_c(values["id"]),
            asset_class=AssetClassParser.from_str(values["asset_class"]),
            quote_currency=Currency.from_str_c(values["quote_currency"]),
            price_precision=values["price_precision"],
            size_precision=values["size_precision"],
            price_increment=Price.from_str_c(values["price_increment"]),
            size_increment=Quantity.from_str_c(values["size_increment"]),
            multiplier=Quantity.from_str_c(values["multiplier"]),
            lot_size=Quantity.from_str_c(values["lot_size"]),
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
            info=json.loads(info) if info is not None else None,
        )

    @staticmethod
    cdef dict to_dict_c(CFDInstrument obj):
        return {
            "type": "CFDInstrument",
            "id": obj.id.value,
            "asset_class": AssetClassParser.to_str(obj.asset_class),
            "quote_currency": obj.quote_currency.code,
            "price_precision": obj.price_precision,
            "price_increment": str(obj.price_increment),
            "size_precision": obj.size_precision,
            "size_increment": str(obj.size_increment),
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
            "ts_event_ns": obj.ts_event_ns,
            "ts_recv_ns": obj.ts_recv_ns,
            "info": json.dumps(obj.info) if obj.info is not None else None,
        }

    @staticmethod
    def from_dict(dict values) -> CFDInstrument:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        CFDInstrument

        """
        return CFDInstrument.from_dict_c(values)

    @staticmethod
    def to_dict(CFDInstrument obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return CFDInstrument.to_dict_c(obj)
