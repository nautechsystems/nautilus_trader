# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import AssetClass
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.core.nautilus_pyo3 import Venue
from nautilus_trader.model.instruments import Commodity
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_COMMODITY = TestInstrumentProviderPyo3.commodity()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.commodity()
    item_2 = TestInstrumentProviderPyo3.commodity()
    assert item_1 == item_2


def test_hash():
    assert hash(_COMMODITY) == hash(_COMMODITY)


def test_to_dict():
    result = _COMMODITY.to_dict()
    assert nautilus_pyo3.Commodity.from_dict(result) == _COMMODITY
    assert result == {
        "type": "Commodity",
        "id": "CL.NYMEX",
        "raw_symbol": "CL",
        "asset_class": "COMMODITY",
        "quote_currency": "USD",
        "price_precision": 2,
        "size_precision": 0,
        "price_increment": "0.01",
        "size_increment": "1",
        "lot_size": "1",
        "max_quantity": None,
        "min_quantity": None,
        "max_notional": None,
        "min_notional": None,
        "max_price": None,
        "min_price": None,
        "margin_init": "0",
        "margin_maint": "0",
        "maker_fee": "0",
        "taker_fee": "0",
        "ts_event": 0,
        "ts_init": 0,
        "info": {},
    }


def test_pyo3_cython_conversion():
    commodity_pyo3 = TestInstrumentProviderPyo3.commodity()
    commodity_pyo3_dict = commodity_pyo3.to_dict()
    commodity_cython = Commodity.from_pyo3(commodity_pyo3)
    commodity_cython_dict = Commodity.to_dict(commodity_cython)
    del commodity_cython_dict["tick_scheme_name"]
    commodity_pyo3_back = nautilus_pyo3.Commodity.from_dict(commodity_cython_dict)
    assert commodity_cython_dict == commodity_pyo3_dict
    assert commodity_pyo3 == commodity_pyo3_back


def test_pyo3_cython_conversion_with_optional_fields():
    commodity_pyo3 = nautilus_pyo3.Commodity(
        instrument_id=InstrumentId(symbol=Symbol("GC"), venue=Venue("COMEX")),
        raw_symbol=Symbol("GC"),
        asset_class=AssetClass.COMMODITY,
        quote_currency=Currency.from_str("USD"),
        price_precision=2,
        size_precision=0,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_int(1),
        lot_size=Quantity.from_int(100),
        max_quantity=Quantity.from_int(10000),
        min_quantity=Quantity.from_int(1),
        max_notional=Money(1_000_000.0, Currency.from_str("USD")),
        min_notional=Money(100.0, Currency.from_str("USD")),
        max_price=Price.from_str("5000.00"),
        min_price=Price.from_str("1.00"),
        margin_init=Decimal("0.10"),
        margin_maint=Decimal("0.05"),
        maker_fee=Decimal("0.001"),
        taker_fee=Decimal("0.002"),
        ts_event=0,
        ts_init=0,
    )

    commodity_cython = Commodity.from_pyo3(commodity_pyo3)

    assert commodity_cython.lot_size.as_double() == 100.0
    assert commodity_cython.max_quantity.as_double() == 10000.0
    assert commodity_cython.min_quantity.as_double() == 1.0
    assert commodity_cython.margin_init == Decimal("0.10")
    assert commodity_cython.margin_maint == Decimal("0.05")
    assert commodity_cython.maker_fee == Decimal("0.001")
    assert commodity_cython.taker_fee == Decimal("0.002")

    commodity_cython_dict = Commodity.to_dict(commodity_cython)
    del commodity_cython_dict["tick_scheme_name"]
    commodity_pyo3_back = nautilus_pyo3.Commodity.from_dict(commodity_cython_dict)
    assert commodity_pyo3_back == commodity_pyo3
