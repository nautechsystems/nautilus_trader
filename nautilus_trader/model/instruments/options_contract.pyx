# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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


cdef class OptionsContract(Instrument):
    """
    Represents a generic options contract instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    raw_symbol : Symbol
        The raw/local/native symbol for the instrument, assigned by the venue.
    asset_class : AssetClass
        The options contract asset class.
    currency : Currency
        The options contract currency.
    price_precision : int
        The price decimal precision.
    price_increment : Price
        The minimum price increment (tick size).
    multiplier : Quantity
        The option multiplier.
    lot_size : Quantity
        The rounded lot unit size (standard/board).
    underlying : str
        The underlying asset.
    option_kind : OptionKind
        The kind of option (PUT | CALL).
    strike_price : Price
        The option strike price.
    activation_ns : uint64_t
        The UNIX timestamp (nanoseconds) for contract activation.
    expiration_ns : uint64_t
        The UNIX timestamp (nanoseconds) for contract expiration.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.
    exchange : str, optional
        The exchange ISO 10383 Market Identifier Code (MIC) where the instrument trades.
    info : dict[str, object], optional
        The additional instrument information.

    Raises
    ------
    ValueError
        If `multiplier` is not positive (> 0).
    ValueError
        If `price_precision` is negative (< 0).
    ValueError
        If `tick_size` is not positive (> 0).
    ValueError
        If `lot_size` is not positive (> 0).
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Symbol raw_symbol not None,
        AssetClass asset_class,
        Currency currency not None,
        int price_precision,
        Price price_increment not None,
        Quantity multiplier not None,
        Quantity lot_size not None,
        str underlying,
        OptionKind option_kind,
        uint64_t activation_ns,
        uint64_t expiration_ns,
        Price strike_price not None,
        uint64_t ts_event,
        uint64_t ts_init,
        str exchange = None,
        dict info = None,
    ):
        Condition.positive_int(multiplier, "multiplier")
        if exchange is not None:
            Condition.valid_string(exchange, "exchange")
        super().__init__(
            instrument_id=instrument_id,
            raw_symbol=raw_symbol,
            asset_class=asset_class,
            instrument_class=InstrumentClass.OPTION,
            quote_currency=currency,
            is_inverse=False,
            price_precision=price_precision,
            size_precision=0,  # No fractional contracts
            price_increment=price_increment,
            size_increment=Quantity.from_int_c(1),
            multiplier=multiplier,
            lot_size=lot_size,
            max_quantity=None,
            min_quantity=Quantity.from_int_c(1),
            max_notional=None,
            min_notional=None,
            max_price=None,
            min_price=None,
            margin_init=Decimal(0),
            margin_maint=Decimal(0),
            maker_fee=Decimal(0),
            taker_fee=Decimal(0),
            ts_event=ts_event,
            ts_init=ts_init,
            info=info,
        )
        self.exchange = exchange
        self.underlying = underlying
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
            f"exchange={self.exchange}, "
            f"quote_currency={self.quote_currency}, "
            f"underlying={self.underlying}, "
            f"option_kind={option_kind_to_str(self.option_kind)}, "
            f"strike_price={self.strike_price}, "
            f"activation={format_iso8601(self.activation_utc)}, "
            f"expiration={format_iso8601(self.expiration_utc)}, "
            f"price_precision={self.price_precision}, "
            f"price_increment={self.price_increment}, "
            f"multiplier={self.multiplier}, "
            f"lot_size={self.lot_size}, "
            f"margin_init={self.margin_init}, "
            f"margin_maint={self.margin_maint}, "
            f"maker_fee={self.maker_fee}, "
            f"taker_fee={self.taker_fee}, "
            f"info={self.info})"
        )

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
        Return the contract expriation timestamp (UTC).

        Returns
        -------
        pd.Timestamp
            tz-aware UTC.

        """
        return pd.Timestamp(self.expiration_ns, tz=pytz.utc)

    @staticmethod
    cdef OptionsContract from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OptionsContract(
            instrument_id=InstrumentId.from_str_c(values["id"]),
            raw_symbol=Symbol(values["raw_symbol"]),
            asset_class=asset_class_from_str(values["asset_class"]),
            currency=Currency.from_str_c(values["currency"]),
            price_precision=values["price_precision"],
            price_increment=Price.from_str(values["price_increment"]),
            multiplier=Quantity.from_str(values["multiplier"]),
            lot_size=Quantity.from_str(values["lot_size"]),
            underlying=values["underlying"],
            option_kind=option_kind_from_str(values["option_kind"]),
            activation_ns=values["activation_ns"],
            expiration_ns=values["expiration_ns"],
            strike_price=Price.from_str(values["strike_price"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            exchange=values["exchange"],
            info=values.get("info"),
        )

    @staticmethod
    cdef dict to_dict_c(OptionsContract obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OptionsContract",
            "id": obj.id.to_str(),
            "raw_symbol": obj.raw_symbol.to_str(),
            "asset_class": asset_class_to_str(obj.asset_class),
            "currency": obj.quote_currency.code,
            "price_precision": obj.price_precision,
            "price_increment": str(obj.price_increment),
            "size_precision": obj.size_precision,
            "size_increment": str(obj.size_increment),
            "multiplier": str(obj.multiplier),
            "max_quantity": str(obj.max_quantity) if obj.max_quantity is not None else None,
            "min_quantity": str(obj.min_quantity) if obj.min_quantity is not None else None,
            "max_price": str(obj.max_price) if obj.max_price is not None else None,
            "min_price": str(obj.min_price) if obj.min_price is not None else None,
            "lot_size": str(obj.lot_size),
            "underlying": str(obj.underlying),
            "option_kind": option_kind_to_str(obj.option_kind),
            "activation_ns": obj.activation_ns,
            "expiration_ns": obj.expiration_ns,
            "strike_price": str(obj.strike_price),
            "margin_init": str(obj.margin_init),
            "margin_maint": str(obj.margin_maint),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "exchange": obj.exchange,
            "info": obj.info,
        }

    @staticmethod
    cdef OptionsContract from_pyo3_c(pyo3_instrument):
        Condition.not_none(pyo3_instrument, "pyo3_instrument")
        return OptionsContract(
            instrument_id=InstrumentId.from_str_c(pyo3_instrument.id.value),
            raw_symbol=Symbol(pyo3_instrument.raw_symbol.value),
            asset_class=asset_class_from_str(str(pyo3_instrument.asset_class)),
            currency=Currency.from_str_c(pyo3_instrument.currency.code),
            price_precision=pyo3_instrument.price_precision,
            price_increment=Price.from_raw_c(pyo3_instrument.price_increment.raw, pyo3_instrument.price_precision),
            multiplier=Quantity.from_raw_c(pyo3_instrument.multiplier.raw, 0),
            lot_size=Quantity.from_raw_c(pyo3_instrument.lot_size.raw, 0),
            underlying=pyo3_instrument.underlying,
            option_kind=option_kind_from_str(str(pyo3_instrument.option_kind)),
            activation_ns=pyo3_instrument.activation_ns,
            expiration_ns=pyo3_instrument.expiration_ns,
            strike_price=Price.from_raw_c(pyo3_instrument.strike_price.raw, pyo3_instrument.strike_price.precision),
            info=pyo3_instrument.info,
            ts_event=pyo3_instrument.ts_event,
            ts_init=pyo3_instrument.ts_init,
            exchange=pyo3_instrument.exchange,
        )

    @staticmethod
    def from_dict(dict values) -> OptionsContract:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        OptionsContract

        """
        return OptionsContract.from_dict_c(values)

    @staticmethod
    def to_dict(OptionsContract obj) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OptionsContract.to_dict_c(obj)

    @staticmethod
    def from_pyo3(pyo3_instrument) -> OptionsContract:
        """
        Return legacy Cython options contract instrument converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_instrument : nautilus_pyo3.OptionsContract
            The pyo3 Rust options contract instrument to convert from.

        Returns
        -------
        OptionsContract

        """
        return OptionsContract.from_pyo3_c(pyo3_instrument)
