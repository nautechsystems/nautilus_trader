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

from nautilus_trader.core.nautilus_pyo3 import Equity
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.model.instruments import Equity as LegacyEquity
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
    assert Equity.from_dict(result) == _AAPL_EQUITY
    assert result == {
        "type": "Equity",
        "id": "AAPL.XNAS",
        "raw_symbol": "AAPL",
        "isin": "US0378331005",
        "currency": "USD",
        "price_precision": 2,
        "price_increment": "0.01",
        "lot_size": "100",
        "max_quantity": None,
        "min_quantity": None,
        "max_price": None,
        "min_price": None,
        "ts_event": 0,
        "ts_init": 0,
    }


def test_legacy_equity_from_pyo3():
    equity = LegacyEquity.from_pyo3(_AAPL_EQUITY)

    assert equity.id.value == "AAPL.XNAS"
