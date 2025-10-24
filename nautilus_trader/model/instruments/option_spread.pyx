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
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.base cimport Price
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Quantity


cdef class OptionSpread(Instrument):
    """
    Represents a generic option spread instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    raw_symbol : Symbol
        The raw/local/native symbol for the instrument, assigned by the venue.
    asset_class : AssetClass
        The option spread asset class.
    currency : Currency
        The option spread currency.
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
    strategy_type : str
        The strategy type of the spread.
    activation_ns : uint64_t
        UNIX timestamp (nanoseconds) for contract activation.
    expiration_ns : uint64_t
        UNIX timestamp (nanoseconds) for contract expiration.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.
    margin_init : Decimal, optional
        The initial (order) margin requirement in percentage of order value.
    margin_maint : Decimal, optional
        The maintenance (position) margin in percentage of position value.
    maker_fee : Decimal, optional
        The fee rate for liquidity makers as a percentage of order value.
    taker_fee : Decimal, optional
        The fee rate for liquidity takers as a percentage of order value.
    exchange : str, optional
        The exchange ISO 10383 Market Identifier Code (MIC) where the instrument trades.
    tick_scheme_name : str, optional
        The name of the tick scheme.
    info : dict[str, object], optional
        The additional instrument information.

    Raises
    ------
    ValueError
        If `strategy_type` is not a valid string.
    ValueError
        If `multiplier` is not positive (> 0).
    ValueError
        If `price_precision` is negative (< 0).
    ValueError
        If `tick_size` is not positive (> 0).
    ValueError
        If `lot_size` is not positive (> 0).
    ValueError
        If `margin_init` is negative (< 0).
    ValueError
        If `margin_maint` is negative (< 0).
    ValueError
        If `exchange` is not ``None`` and not a valid string.

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
        str strategy_type,
        uint64_t activation_ns,
        uint64_t expiration_ns,
        uint64_t ts_event,
        uint64_t ts_init,
        margin_init: Decimal | None = None,
        margin_maint: Decimal | None = None,
        maker_fee: Decimal | None = None,
        taker_fee: Decimal | None = None,
        str exchange = None,
        str tick_scheme_name = None,
        dict info = None,
    ) -> None:
        Condition.valid_string(strategy_type, "strategy_type")

        if exchange is not None:
            Condition.valid_string(exchange, "exchange")

        super().__init__(
            instrument_id=instrument_id,
            raw_symbol=raw_symbol,
            asset_class=asset_class,
            instrument_class=InstrumentClass.OPTION_SPREAD,
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
            margin_init=margin_init or Decimal(0),
            margin_maint=margin_maint or Decimal(0),
            maker_fee=maker_fee or Decimal(0),
            taker_fee=taker_fee or Decimal(0),
            ts_event=ts_event,
            ts_init=ts_init,
            tick_scheme_name=tick_scheme_name,
            info=info,
        )

        self.exchange = exchange
        self.underlying = underlying
        self.strategy_type = strategy_type
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
            f"currency={self.quote_currency}, "
            f"underlying={self.underlying}, "
            f"strategy_type={self.strategy_type}, "
            f"activation={format_iso8601(self.activation_utc, nanos_precision=False)}, "
            f"expiration={format_iso8601(self.expiration_utc, nanos_precision=False)}, "
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
        Return the contract expiration timestamp (UTC).

        Returns
        -------
        pd.Timestamp
            tz-aware UTC.

        """
        return pd.Timestamp(self.expiration_ns, tz=pytz.utc)

    @staticmethod
    cdef OptionSpread from_dict_c(dict values):
        Condition.not_none(values, "values")
        return OptionSpread(
            instrument_id=InstrumentId.from_str_c(values["id"]),
            raw_symbol=Symbol(values["raw_symbol"]),
            asset_class=asset_class_from_str(values["asset_class"]),
            currency=Currency.from_str_c(values["currency"]),
            price_precision=values["price_precision"],
            price_increment=Price.from_str(values["price_increment"]),
            multiplier=Quantity.from_str(values["multiplier"]),
            lot_size=Quantity.from_str(values["lot_size"]),
            underlying=values["underlying"],
            strategy_type=values["strategy_type"],
            activation_ns=values["activation_ns"],
            expiration_ns=values["expiration_ns"],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            margin_init=Decimal(values["margin_init"]),
            margin_maint=Decimal(values["margin_maint"]),
            maker_fee=Decimal(values["maker_fee"]),
            taker_fee=Decimal(values["taker_fee"]),
            exchange=values["exchange"],
            tick_scheme_name=values.get("tick_scheme_name"),
            info=values.get("info"),
        )

    @staticmethod
    cdef dict to_dict_c(OptionSpread obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "OptionSpread",
            "id": obj.id.to_str(),
            "raw_symbol": obj.raw_symbol.to_str(),
            "asset_class": asset_class_to_str(obj.asset_class),
            "strategy_type": obj.strategy_type,
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
            "activation_ns": obj.activation_ns,
            "expiration_ns": obj.expiration_ns,
            "margin_init": str(obj.margin_init),
            "margin_maint": str(obj.margin_maint),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "margin_init": str(obj.margin_init),
            "margin_maint": str(obj.margin_maint),
            "maker_fee": str(obj.maker_fee),
            "taker_fee": str(obj.taker_fee),
            "exchange": obj.exchange,
            "tick_scheme_name": obj.tick_scheme_name,
            "info": obj.info,
        }

    @staticmethod
    cdef OptionSpread from_pyo3_c(pyo3_instrument):
        Condition.not_none(pyo3_instrument, "pyo3_instrument")
        return OptionSpread(
            instrument_id=InstrumentId.from_str_c(pyo3_instrument.id.value),
            raw_symbol=Symbol(pyo3_instrument.raw_symbol.value),
            asset_class=asset_class_from_str(str(pyo3_instrument.asset_class)),
            currency=Currency.from_str_c(pyo3_instrument.currency.code),
            price_precision=pyo3_instrument.price_precision,
            price_increment=Price.from_raw_c(pyo3_instrument.price_increment.raw, pyo3_instrument.price_precision),
            multiplier=Quantity.from_raw_c(pyo3_instrument.multiplier.raw, pyo3_instrument.multiplier.precision),
            lot_size=Quantity.from_raw_c(pyo3_instrument.lot_size.raw, pyo3_instrument.lot_size.precision),
            underlying=pyo3_instrument.underlying,
            strategy_type=pyo3_instrument.strategy_type,
            activation_ns=pyo3_instrument.activation_ns,
            expiration_ns=pyo3_instrument.expiration_ns,
            margin_init=Decimal(pyo3_instrument.margin_init),
            margin_maint=Decimal(pyo3_instrument.margin_maint),
            maker_fee=Decimal(pyo3_instrument.maker_fee),
            taker_fee=Decimal(pyo3_instrument.taker_fee),
            exchange=pyo3_instrument.exchange,
            ts_event=pyo3_instrument.ts_event,
            ts_init=pyo3_instrument.ts_init,
            info=pyo3_instrument.info,
        )

    @staticmethod
    def from_dict(dict values) -> OptionSpread:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        OptionSpread

        """
        return OptionSpread.from_dict_c(values)

    @staticmethod
    def to_dict(OptionSpread obj) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return OptionSpread.to_dict_c(obj)

    @staticmethod
    def from_pyo3(pyo3_instrument) -> OptionSpread:
        """
        Return legacy Cython option spread instrument converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_instrument : nautilus_pyo3.OptionSpread
            The pyo3 Rust option spread instrument to convert from.

        Returns
        -------
        OptionSpread

        """
        return OptionSpread.from_pyo3_c(pyo3_instrument)
