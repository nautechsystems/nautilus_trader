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

from nautilus_trader.core.nautilus_pyo3 import FuturesSpread
from nautilus_trader.model.instruments import FuturesSpread as LegacyFuturesSpread
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_ES_FUTURES_SPREAD = TestInstrumentProviderPyo3.futures_spread_es()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.futures_spread_es()
    item_2 = TestInstrumentProviderPyo3.futures_spread_es()
    assert item_1 == item_2


def test_hash():
    assert hash(_ES_FUTURES_SPREAD) == hash(_ES_FUTURES_SPREAD)


def test_to_dict():
    result = _ES_FUTURES_SPREAD.to_dict()
    assert FuturesSpread.from_dict(result) == _ES_FUTURES_SPREAD
    assert result == {
        "type": "FuturesSpread",
        "id": "ESM4-ESU4.GLBX",
        "raw_symbol": "ESM4-ESU4",
        "asset_class": "INDEX",
        "underlying": "ES",
        "strategy_type": "EQ",
        "activation_ns": 1655818200000000000,
        "expiration_ns": 1718976600000000000,
        "currency": "USD",
        "price_precision": 2,
        "price_increment": "0.01",
        "multiplier": "1",
        "lot_size": "1",
        "max_price": None,
        "max_quantity": None,
        "min_price": None,
        "min_quantity": None,
        "ts_event": 0,
        "ts_init": 0,
        "exchange": "XCME",
    }


def test_legacy_futures_contract_from_pyo3():
    future = LegacyFuturesSpread.from_pyo3(_ES_FUTURES_SPREAD)

    assert future.id.value == "ESM4-ESU4.GLBX"
