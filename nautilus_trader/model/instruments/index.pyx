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


cdef class IndexInstrument(Instrument):
    """
    Represents a generic index instrument.

    This instrument is *typically* not tradable.

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
    price_increment : Price
        The minimum price increment (tick size).
    size_precision : int
        The trading size decimal precision.
    size_increment : Quantity
        The minimum size increment.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.
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

    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        Symbol raw_symbol not None,
        Currency currency not None,
        int price_precision,
        int size_precision,
        Price price_increment not None,
        Quantity size_increment not None,
        uint64_t ts_event,
        uint64_t ts_init,
        dict info = None,
    ) -> None:
        super().__init__(
            instrument_id=instrument_id,
            raw_symbol=raw_symbol,
            asset_class=AssetClass.INDEX,
            instrument_class=InstrumentClass.FUTURE,
            quote_currency=currency,
            is_inverse=False,
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=price_increment,
            size_increment=size_increment,
            multiplier=Quantity.from_int_c(1),
            ts_event=ts_event,
            ts_init=ts_init,
            info=info,
            margin_init=Decimal(0),
            margin_maint=Decimal(0),
            maker_fee=Decimal(0),
            taker_fee=Decimal(0),
        )

    @staticmethod
    cdef IndexInstrument from_dict_c(dict values):
        Condition.not_none(values, "values")
        return IndexInstrument(
            instrument_id=InstrumentId.from_str_c(values["id"]),
            raw_symbol=Symbol(values["raw_symbol"]),
            currency=Currency.from_str_c(values["currency"]),
            price_precision=values["price_precision"],
            price_increment=Price.from_str(values["price_increment"]),
            size_precision=values["size_precision"],
            size_increment=Quantity.from_str(values["size_increment"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            info=values["info"],
        )

    @staticmethod
    cdef dict to_dict_c(IndexInstrument obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "IndexInstrument",
            "id": obj.id.to_str(),
            "raw_symbol": obj.raw_symbol.to_str(),
            "currency": obj.quote_currency.code,
            "price_precision": obj.price_precision,
            "price_increment": str(obj.price_increment),
            "size_precision": obj.size_precision,
            "size_increment": str(obj.size_increment),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "info": obj.info,
        }

    @staticmethod
    cdef IndexInstrument from_pyo3_c(pyo3_instrument):
        return IndexInstrument(
            instrument_id=InstrumentId.from_str_c(pyo3_instrument.id.value),
            raw_symbol=Symbol(pyo3_instrument.id.symbol.value),
            currency=Currency.from_str_c(pyo3_instrument.quote_currency.code),
            price_precision=pyo3_instrument.price_precision,
            price_increment=Price.from_raw_c(pyo3_instrument.price_increment.raw, pyo3_instrument.price_precision),
            size_precision=pyo3_instrument.size_precision,
            size_increment=Quantity.from_raw_c(pyo3_instrument.size_increment.raw, pyo3_instrument.size_precision),
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
        IndexInstrument

        """
        return IndexInstrument.from_dict_c(values)

    @staticmethod
    def to_dict(Instrument obj) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return IndexInstrument.to_dict_c(obj)

    @staticmethod
    def from_pyo3(pyo3_instrument) -> IndexInstrument:
        """
        Return legacy Cython index instrument converted from the given pyo3 Rust object.

        Parameters
        ----------
        pyo3_instrument : nautilus_pyo3.IndexInstrument
            The pyo3 Rust index instrument to convert from.

        Returns
        -------
        IndexInstrument

        """
        return IndexInstrument.from_pyo3_c(pyo3_instrument)
