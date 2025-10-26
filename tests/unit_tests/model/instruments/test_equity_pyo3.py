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

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.model.instruments import Equity
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_AAPL_EQUITY = TestInstrumentProviderPyo3.aapl_equity()


def test_properties():
    assert _AAPL_EQUITY.id == InstrumentId.from_str("AAPL.XNAS")


def test_equality():
    item_1 = TestInstrumentProviderPyo3.aapl_equity()
    item_2 = TestInstrumentProviderPyo3.aapl_equity()
    assert item_1 == item_2


def test_hash():
    assert hash(_AAPL_EQUITY) == hash(_AAPL_EQUITY)


def test_to_dict():
    result = _AAPL_EQUITY.to_dict()
    assert nautilus_pyo3.Equity.from_dict(result) == _AAPL_EQUITY
    assert result == {
        "type": "Equity",
        "id": "AAPL.XNAS",
        "raw_symbol": "AAPL",
        "isin": "US0378331005",
        "currency": "USD",
        "price_precision": 2,
        "price_increment": "0.01",
        "maker_fee": "0",
        "taker_fee": "0",
        "margin_init": "0",
        "margin_maint": "0",
        "lot_size": "100",
        "max_quantity": None,
        "min_quantity": None,
        "max_price": None,
        "min_price": None,
        "ts_event": 0,
        "ts_init": 0,
        "info": {},
    }


def test_legacy_equity_from_pyo3():
    equity = Equity.from_pyo3(_AAPL_EQUITY)

    assert equity.id.value == "AAPL.XNAS"


def test_pyo3_cython_conversion():
    equity_pyo3 = TestInstrumentProviderPyo3.aapl_equity()
    equity_pyo3_dict = equity_pyo3.to_dict()
    equity_cython = Equity.from_pyo3(equity_pyo3)
    equity_cython_dict = Equity.to_dict(equity_cython)
    del equity_cython_dict["tick_scheme_name"]  # TODO: Under development
    equity_pyo3_back = nautilus_pyo3.Equity.from_dict(equity_cython_dict)
    assert equity_cython_dict == equity_pyo3_dict
    assert equity_pyo3 == equity_pyo3_back


def test_pyo3_cython_conversion_with_fees():
    # Arrange
    equity_pyo3 = nautilus_pyo3.Equity(
        instrument_id=InstrumentId.from_str("TEST.XNAS"),
        raw_symbol=nautilus_pyo3.Symbol("TEST"),
        isin=None,
        currency=nautilus_pyo3.Currency.from_str("USD"),
        price_precision=2,
        price_increment=Price.from_str("0.01"),
        lot_size=Quantity.from_int(100),
        max_quantity=Quantity.from_int(10000),
        min_quantity=Quantity.from_int(1),
        max_price=None,
        min_price=None,
        margin_init=Decimal("0.50"),
        margin_maint=Decimal("0.25"),
        maker_fee=Decimal("0.001"),
        taker_fee=Decimal("0.002"),
        ts_event=0,
        ts_init=0,
    )

    # Act
    equity_cython = Equity.from_pyo3(equity_pyo3)

    # Assert
    assert equity_cython.margin_init == Decimal("0.50")
    assert equity_cython.margin_maint == Decimal("0.25")
    assert equity_cython.maker_fee == Decimal("0.001")
    assert equity_cython.taker_fee == Decimal("0.002")
    assert equity_cython.max_quantity.as_double() == 10000.0
    assert equity_cython.min_quantity.as_double() == 1.0

    # Round-trip conversion
    equity_cython_dict = Equity.to_dict(equity_cython)
    del equity_cython_dict["tick_scheme_name"]
    equity_pyo3_back = nautilus_pyo3.Equity.from_dict(equity_cython_dict)
    assert equity_pyo3_back == equity_pyo3
