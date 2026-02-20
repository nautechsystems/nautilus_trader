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
from nautilus_trader.model.instruments import Cfd
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_CFD = TestInstrumentProviderPyo3.cfd()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.cfd()
    item_2 = TestInstrumentProviderPyo3.cfd()
    assert item_1 == item_2


def test_hash():
    assert hash(_CFD) == hash(_CFD)


def test_properties():
    assert _CFD.id == InstrumentId.from_str("AUDUSD.OANDA")


def test_to_dict():
    result = _CFD.to_dict()
    assert nautilus_pyo3.Cfd.from_dict(result) == _CFD
    assert result == {
        "type": "Cfd",
        "id": "AUDUSD.OANDA",
        "raw_symbol": "AUD/USD",
        "asset_class": "FX",
        "quote_currency": "USD",
        "base_currency": "AUD",
        "price_precision": 5,
        "size_precision": 0,
        "price_increment": "0.00001",
        "size_increment": "1",
        "lot_size": "1000",
        "max_quantity": None,
        "min_quantity": None,
        "max_notional": None,
        "min_notional": None,
        "max_price": None,
        "min_price": None,
        "margin_init": "0.03",
        "margin_maint": "0.03",
        "maker_fee": "0.00002",
        "taker_fee": "0.00002",
        "ts_event": 0,
        "ts_init": 0,
        "info": {},
    }


def test_pyo3_cython_conversion():
    cfd_pyo3 = TestInstrumentProviderPyo3.cfd()
    cfd_pyo3_dict = cfd_pyo3.to_dict()
    cfd_cython = Cfd.from_pyo3(cfd_pyo3)
    cfd_cython_dict = Cfd.to_dict(cfd_cython)
    del cfd_cython_dict["tick_scheme_name"]
    cfd_pyo3_back = nautilus_pyo3.Cfd.from_dict(cfd_cython_dict)
    assert cfd_cython_dict == cfd_pyo3_dict
    assert cfd_pyo3 == cfd_pyo3_back


def test_pyo3_cython_conversion_with_optional_fields():
    cfd_pyo3 = nautilus_pyo3.Cfd(
        instrument_id=InstrumentId.from_str("XAUUSD.OANDA"),
        raw_symbol=Symbol("XAU/USD"),
        asset_class=AssetClass.COMMODITY,
        quote_currency=Currency.from_str("USD"),
        base_currency=Currency.from_str("XAU"),
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

    cfd_cython = Cfd.from_pyo3(cfd_pyo3)

    assert cfd_cython.lot_size.as_double() == 100.0
    assert cfd_cython.max_quantity.as_double() == 10000.0
    assert cfd_cython.min_quantity.as_double() == 1.0
    assert cfd_cython.base_currency.code == "XAU"
    assert cfd_cython.margin_init == Decimal("0.10")
    assert cfd_cython.margin_maint == Decimal("0.05")
    assert cfd_cython.maker_fee == Decimal("0.001")
    assert cfd_cython.taker_fee == Decimal("0.002")

    cfd_cython_dict = Cfd.to_dict(cfd_cython)
    del cfd_cython_dict["tick_scheme_name"]
    cfd_pyo3_back = nautilus_pyo3.Cfd.from_dict(cfd_cython_dict)
    assert cfd_pyo3_back == cfd_pyo3
