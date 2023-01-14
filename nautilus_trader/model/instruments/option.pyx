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

from cpython.datetime cimport date
from libc.stdint cimport uint64_t

from decimal import Decimal

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.enums_c cimport AssetClass
from nautilus_trader.model.enums_c cimport AssetType
from nautilus_trader.model.enums_c cimport OptionKind
from nautilus_trader.model.enums_c cimport asset_class_from_str
from nautilus_trader.model.enums_c cimport asset_class_to_str
from nautilus_trader.model.enums_c cimport option_kind_from_str
from nautilus_trader.model.enums_c cimport option_kind_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.base cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Option(Instrument):
    """
    Represents a generic Options Contract instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    native_symbol : Symbol
        The native/local symbol on the exchange for the instrument.
    asset_class : AssetClass
        The futures contract asset class.
    currency : Currency
        The futures contract currency.
    price_precision : int
        The price decimal precision.
    price_increment : Price
        The minimum price increment (tick size).
    multiplier : Quantity
        The option multiplier.
    lot_size : Quantity
        The rounded lot unit size (standard/board).
    strike_price : Price
        The option strike price.
    underlying : str
        The underlying asset.
    expiry_date : date
        The option expiry date.
    ts_event : uint64_t
        The UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        The UNIX timestamp (nanoseconds) when the data object was initialized.

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
        Symbol native_symbol not None,
        AssetClass asset_class,
        Currency currency not None,
        int price_precision,
        Price price_increment not None,
        Quantity multiplier not None,
        Quantity lot_size not None,
        Price strike_price not None,
        str underlying,
        date expiry_date,
        OptionKind kind,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        Condition.positive_int(multiplier, "multiplier")
        super().__init__(
            instrument_id=instrument_id,
            native_symbol=native_symbol,
            asset_class=asset_class,
            asset_type=AssetType.OPTION,
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
            info={},
        )
        self.underlying = underlying
        self.expiry_date = expiry_date
        self.strike_price = strike_price
        self.kind = kind

    @staticmethod
    cdef Option from_dict_c(dict values):
        Condition.not_none(values, "values")
        return Option(
            instrument_id=InstrumentId.from_str_c(values["id"]),
            native_symbol=Symbol(values["native_symbol"]),
            asset_class=asset_class_from_str(values["asset_class"]),
            currency=Currency.from_str_c(values["currency"]),
            price_precision=values["price_precision"],
            price_increment=Price.from_str(values["price_increment"]),
            multiplier=Quantity.from_str(values["multiplier"]),
            lot_size=Quantity.from_str(values["lot_size"]),
            underlying=values['underlying'],
            expiry_date=date.fromisoformat(values["expiry_date"]),
            strike_price=Price.from_str(values["strike_price"]),
            kind=option_kind_from_str(values["kind"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(Option obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "Option",
            "id": obj.id.to_str(),
            "native_symbol": obj.native_symbol.to_str(),
            "asset_class": asset_class_to_str(obj.asset_class),
            "currency": obj.quote_currency.code,
            "price_precision": obj.price_precision,
            "price_increment": str(obj.price_increment),
            "size_precision": obj.size_precision,
            "size_increment": str(obj.size_increment),
            "multiplier": str(obj.multiplier),
            "lot_size": str(obj.lot_size),
            "underlying": str(obj.underlying),
            "expiry_date": obj.expiry_date.isoformat(),
            "strike_price": str(obj.strike_price),
            "margin_init": str(obj.margin_init),
            "margin_maint": str(obj.margin_maint),
            "kind": option_kind_to_str(obj.kind),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> Option:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        Option

        """
        return Option.from_dict_c(values)

    @staticmethod
    def to_dict(Option obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return Option.to_dict_c(obj)
