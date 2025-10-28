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

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport AssetClass
from nautilus_trader.core.rust.model cimport InstrumentClass
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Equity(Instrument):
    """
    Represents a generic equity instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    raw_symbol : Symbol
        The raw/local/native symbol for the instrument, assigned by the venue.
    currency : Currency
        The futures contract currency.
    price_precision : int
        The price decimal precision.
    price_increment : Decimal
        The minimum price increment (tick size).
    lot_size : Quantity
        The rounded lot unit size (standard/board).
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
    isin : str, optional
        The instruments International Securities Identification Number (ISIN).
    tick_scheme_name : str, optional
        The name of the tick scheme.
    info : dict[str, object], optional
        The additional instrument information.

    Raises
    ------
    ValueError
        If `price_precision` is negative (< 0).
    ValueError
        If `price_increment` is not positive (> 0).
    ValueError
        If `lot_size` is not positive (> 0).
    ValueError
        If `margin_init` is negative (< 0).
    ValueError
        If `margin_maint` is negative (< 0).
    ValueError
        If `isin` is not ``None`` and not a valid string.

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Symbol raw_symbol not None,
        Currency currency not None,
        int price_precision,
        Price price_increment not None,
        Quantity lot_size not None,
        uint64_t ts_event,
        uint64_t ts_init,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        margin_init: Decimal | None = None,
        margin_maint: Decimal | None = None,
        maker_fee: Decimal | None = None,
        taker_fee: Decimal | None = None,
        str isin: str | None = None,
        str tick_scheme_name = None,
        dict info = None,
    ) -> None:
        if isin is not None:
            Condition.valid_string(isin, "isin")
        super().__init__(
            instrument_id=instrument_id,
            raw_symbol=raw_symbol,
            asset_class=AssetClass.EQUITY,
            instrument_class=InstrumentClass.SPOT,
            quote_currency=currency,
            is_inverse=False,
            price_precision=price_precision,
            size_precision=0,  # No fractional units
            price_increment=price_increment,
            size_increment=Quantity.from_int_c(1),
            multiplier=Quantity.from_int_c(1),
            lot_size=lot_size,
            max_quantity=max_quantity,
            min_quantity=min_quantity,
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

        self.isin = isin

    @staticmethod
    cdef Equity from_dict_c(dict values):
        Condition.not_none(values, "values")
        return Equity(
            instrument_id=InstrumentId.from_str_c(values["id"]),
            raw_symbol=Symbol(values["raw_symbol"]),
            currency=Currency.from_str_c(values["currency"]),
            price_precision=values["price_precision"],
            price_increment=Price.from_str(values["price_increment"]),
            lot_size=Quantity.from_str(values["lot_size"]),
            isin=values.get("isin"),  # Can be None,
            margin_init=Decimal(values.get("margin_init", 0)) if values.get("margin_init") is not None else None,
            margin_maint=Decimal(values.get("margin_maint", 0)) if values.get("margin_maint") is not None else None,
            maker_fee=Decimal(values.get("maker_fee", 0)) if values.get("maker_fee") is not None else None,
            taker_fee=Decimal(values.get("taker_fee", 0))  if values.get("taker_fee") is not None else None,
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            tick_scheme_name=values.get("tick_scheme_name"),
            info=values["info"],
        )

    @staticmethod
    cdef dict to_dict_c(Equity obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "Equity",
            "id": obj.id.to_str(),
            "raw_symbol": obj.raw_symbol.to_str(),
            "currency": obj.quote_currency.code,
            "price_precision": obj.price_precision,
            "price_increment": str(obj.price_increment),
            "lot_size": str(obj.lot_size),
            "isin": obj.isin,
            "margin_init": str(obj.margin_init),
            "margin_maint": str(obj.margin_maint),
            "maker_fee": str(obj.maker_fee),
            "taker_fee": str(obj.taker_fee),
            "min_price": str(obj.min_price) if obj.min_price is not None else None,
            "max_price": str(obj.max_price) if obj.max_price is not None else None,
            "max_quantity": str(obj.max_quantity) if obj.max_quantity is not None else None,
            "min_quantity": str(obj.min_quantity) if obj.min_quantity is not None else None,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "tick_scheme_name": obj.tick_scheme_name,
            "info": obj.info,
        }

    @staticmethod
    cdef Equity from_pyo3_c(pyo3_instrument):
        return Equity(
            instrument_id=InstrumentId.from_str_c(pyo3_instrument.id.value),
            raw_symbol=Symbol(pyo3_instrument.raw_symbol.value),
            currency=Currency.from_str_c(pyo3_instrument.quote_currency.code),
            price_precision=pyo3_instrument.price_precision,
            price_increment=Price.from_raw_c(pyo3_instrument.price_increment.raw, pyo3_instrument.price_precision),
            lot_size=Quantity.from_raw_c(pyo3_instrument.lot_size.raw, pyo3_instrument.lot_size.precision) if pyo3_instrument.lot_size is not None else Quantity.from_int_c(1),
            max_quantity=Quantity.from_raw_c(pyo3_instrument.max_quantity.raw, pyo3_instrument.max_quantity.precision) if pyo3_instrument.max_quantity is not None else None,
            min_quantity=Quantity.from_raw_c(pyo3_instrument.min_quantity.raw, pyo3_instrument.min_quantity.precision) if pyo3_instrument.min_quantity is not None else None,
            margin_init=Decimal(pyo3_instrument.margin_init),
            margin_maint=Decimal(pyo3_instrument.margin_maint),
            maker_fee=Decimal(pyo3_instrument.maker_fee),
            taker_fee=Decimal(pyo3_instrument.taker_fee),
            isin=pyo3_instrument.isin,
            ts_event=pyo3_instrument.ts_event,
            ts_init=pyo3_instrument.ts_init,
            info=pyo3_instrument.info,
        )

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
        Equity

        """
        return Equity.from_dict_c(values)

    @staticmethod
    def to_dict(Instrument obj) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return Equity.to_dict_c(obj)

    @staticmethod
    def from_pyo3(pyo3_instrument) -> Equity:
        """
        Return legacy Cython equity instrument converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_instrument : nautilus_pyo3.Equity
            The pyo3 Rust equity instrument to convert from.

        Returns
        -------
        Equity

        """
        return Equity.from_pyo3_c(pyo3_instrument)
