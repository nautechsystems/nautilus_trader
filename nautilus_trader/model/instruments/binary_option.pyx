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
from nautilus_trader.model.objects cimport Quantity


cdef class BinaryOption(Instrument):
    """
    Represents a generic binary option instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    raw_symbol : Symbol
        The raw/local/native symbol for the instrument, assigned by the venue.
    asset_class : AssetClass
        The option contract asset class.
    currency : Currency
        The option contract currency.
    price_precision : int
        The price decimal precision.
    size_precision : int
        The trading size decimal precision.
    price_increment : Price
        The minimum price increment (tick size).
    size_increment : Quantity
        The minimum size increment.
    activation_ns : uint64_t
        UNIX timestamp (nanoseconds) for contract activation.
    expiration_ns : uint64_t
        UNIX timestamp (nanoseconds) for contract expiration.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.
    max_quantity : Quantity, optional
        The maximum allowable order quantity.
    min_quantity : Quantity, optional
        The minimum allowable order quantity.
    maker_fee : Decimal, optional
        The fee rate for liquidity makers as a percentage of order value.
    taker_fee : Decimal, optional
        The fee rate for liquidity takers as a percentage of order value.
    outcome : str, optional
        The binary outcome of the market.
    description : str, optional
        The market description.
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

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Symbol raw_symbol not None,
        AssetClass asset_class,
        Currency currency not None,
        int price_precision,
        int size_precision,
        Price price_increment not None,
        Quantity size_increment not None,
        uint64_t activation_ns,
        uint64_t expiration_ns,
        uint64_t ts_event,
        uint64_t ts_init,
        Quantity max_quantity: Quantity | None = None,
        Quantity min_quantity: Quantity | None = None,
        maker_fee: Decimal | None = None,
        taker_fee: Decimal | None = None,
        str outcome = None,
        str description = None,
        str tick_scheme_name = None,
        dict info = None,
    ) -> None:
        if description is not None:
            Condition.valid_string(description, "description")
        super().__init__(
            instrument_id=instrument_id,
            raw_symbol=raw_symbol,
            asset_class=asset_class,
            instrument_class=InstrumentClass.BINARY_OPTION,
            quote_currency=currency,
            is_inverse=False,
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=price_increment,
            size_increment=size_increment,
            multiplier=Quantity.from_int_c(1),
            lot_size=Quantity.from_int_c(1),
            max_quantity=max_quantity,
            min_quantity=min_quantity,
            max_notional=None,
            min_notional=None,
            max_price=None,
            min_price=None,
            margin_init=Decimal(0),
            margin_maint=Decimal(0),
            maker_fee=maker_fee or Decimal(0),
            taker_fee=taker_fee or Decimal(0),
            ts_event=ts_event,
            ts_init=ts_init,
            tick_scheme_name=tick_scheme_name,
            info=info,
        )

        self.outcome = outcome
        self.description = description
        self.activation_ns = activation_ns
        self.expiration_ns = expiration_ns

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
    cdef BinaryOption from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef str max_q = values["max_quantity"]
        cdef str min_q = values["min_quantity"]
        return BinaryOption(
            instrument_id=InstrumentId.from_str_c(values["id"]),
            raw_symbol=Symbol(values["raw_symbol"]),
            asset_class=asset_class_from_str(values["asset_class"]),
            currency=Currency.from_str_c(values["currency"]),
            price_precision=values["price_precision"],
            size_precision=values["size_precision"],
            price_increment=Price.from_str(values["price_increment"]),
            size_increment=Quantity.from_str(values["size_increment"]),
            activation_ns=values["activation_ns"],
            expiration_ns=values["expiration_ns"],
            maker_fee=Decimal(values["maker_fee"]),
            taker_fee=Decimal(values["taker_fee"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            outcome=values["outcome"],
            description=values["description"],
            max_quantity=Quantity.from_str(max_q) if max_q is not None else None,
            min_quantity=Quantity.from_str(min_q) if min_q is not None else None,
            tick_scheme_name=values.get("tick_scheme_name"),
            info=values.get("info"),
        )

    @staticmethod
    cdef dict to_dict_c(BinaryOption obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "BinaryOption",
            "id": obj.id.to_str(),
            "raw_symbol": obj.raw_symbol.to_str(),
            "outcome": obj.outcome,
            "description": obj.description,
            "asset_class": asset_class_to_str(obj.asset_class),
            "currency": obj.quote_currency.code,
            "price_precision": obj.price_precision,
            "size_precision": obj.size_precision,
            "price_increment": str(obj.price_increment),
            "size_increment": str(obj.size_increment),
            "activation_ns": obj.activation_ns,
            "expiration_ns": obj.expiration_ns,
            "max_quantity": str(obj.max_quantity) if obj.max_quantity is not None else None,
            "min_quantity": str(obj.min_quantity) if obj.min_quantity is not None else None,
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
    def from_dict(dict values) -> BinaryOption:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
       BinaryOption

        """
        return BinaryOption.from_dict_c(values)

    @staticmethod
    def to_dict(BinaryOption obj) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return BinaryOption.to_dict_c(obj)
