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

from decimal import Decimal
from typing import Optional

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.enums_c cimport AssetClass
from nautilus_trader.model.enums_c cimport AssetType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Equity(Instrument):
    """
    Represents a generic Equity instrument.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID.
    native_symbol : Symbol
        The native/local symbol on the exchange for the instrument.
    currency : Currency
        The futures contract currency.
    price_precision : int
        The price decimal precision.
    price_increment : Decimal
        The minimum price increment (tick size).
    multiplier : Decimal
        The contract value multiplier (determines tick value).
    lot_size : Quantity
        The rounded lot unit size (standard/board).
    isin : str, optional
        The International Securities Identification Number (ISIN).
    margin_init : Decimal, optional
        The initial (order) margin requirement in percentage of order value.
    margin_maint : Decimal, optional
        The maintenance (position) margin in percentage of position value.
    maker_fee : Decimal, optional
        The fee rate for liquidity makers as a percentage of order value.
    taker_fee : Decimal, optional
        The fee rate for liquidity takers as a percentage of order value.
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
        Currency currency not None,
        int price_precision,
        Price price_increment not None,
        Quantity multiplier not None,
        Quantity lot_size not None,
        uint64_t ts_event,
        uint64_t ts_init,
        str isin: Optional[str] = None,
        margin_init: Optional[Decimal] = None,
        margin_maint: Optional[Decimal] = None,
        maker_fee: Optional[Decimal] = None,
        taker_fee: Optional[Decimal] = None,
    ):
        super().__init__(
            instrument_id=instrument_id,
            native_symbol=native_symbol,
            asset_class=AssetClass.EQUITY,
            asset_type=AssetType.SPOT,
            quote_currency=currency,
            is_inverse=False,
            price_precision=price_precision,
            size_precision=0,  # No fractional units
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
            margin_init=margin_init if margin_init else Decimal(0),
            margin_maint=margin_maint if margin_maint else Decimal(0),
            maker_fee=maker_fee if maker_fee else Decimal(0),
            taker_fee=taker_fee if taker_fee else Decimal(0),
            ts_event=ts_event,
            ts_init=ts_init,
            info={},
        )
        self.isin = isin

    @staticmethod
    cdef Equity from_dict_c(dict values):
        Condition.not_none(values, "values")
        return Equity(
            instrument_id=InstrumentId.from_str_c(values["id"]),
            native_symbol=Symbol(values["native_symbol"]),
            currency=Currency.from_str_c(values["currency"]),
            price_precision=values["price_precision"],
            price_increment=Price.from_str(values["price_increment"]),
            multiplier=Quantity.from_str(values["multiplier"]),
            lot_size=Quantity.from_str(values["lot_size"]),
            isin=values.get("isin"),  # Can be None,
            margin_init=Decimal(values.get("margin_init", 0)),  # Can be None,
            margin_maint=Decimal(values.get("margin_maint", 0)),  # Can be None,
            maker_fee=Decimal(values.get("maker_fee", 0)),  # Can be None,
            taker_fee=Decimal(values.get("taker_fee", 0)),  # Can be None,
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(Equity obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "Equity",
            "id": obj.id.to_str(),
            "native_symbol": obj.native_symbol.to_str(),
            "currency": obj.quote_currency.code,
            "price_precision": obj.price_precision,
            "price_increment": str(obj.price_increment),
            "size_precision": obj.size_precision,
            "size_increment": str(obj.size_increment),
            "multiplier": str(obj.multiplier),
            "lot_size": str(obj.lot_size),
            "isin": obj.isin,
            "margin_init": str(obj.margin_init),
            "margin_maint": str(obj.margin_maint),
            "maker_fee": str(obj.maker_fee),
            "taker_fee": str(obj.taker_fee),
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

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
    def to_dict(Instrument obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return Equity.to_dict_c(obj)
