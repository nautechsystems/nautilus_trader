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

from nautilus_trader.core.nautilus_pyo3 import FuturesContract
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


futures_contract_es = TestInstrumentProviderPyo3.futures_contract_es()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.btcusdt_binance()
    item_2 = TestInstrumentProviderPyo3.btcusdt_binance()
    assert item_1 == item_2


def test_hash():
    assert hash(futures_contract_es) == hash(futures_contract_es)


def test_to_dict():
    result = futures_contract_es.to_dict()
    assert FuturesContract.from_dict(result) == futures_contract_es
    assert result == {
        "type": "FuturesContract",
        "id": "ESZ21.CME",
        "raw_symbol": "ESZ21",
        "asset_class": "INDEX",
        "underlying": "ES",
        "activation_ns": 1631836800000000000,
        "expiration_ns": 1639699200000000000,
        "currency": "USD",
        "price_precision": 2,
        "price_increment": "0.01",
        "maker_fee": 0.001,
        "taker_fee": 0.001,
        "margin_maint": 0.0,
        "margin_init": 0.0,
        "multiplier": "1.0",
        "lot_size": "1.0",
        "max_price": None,
        "max_quantity": None,
        "min_price": None,
        "min_quantity": None,
        "ts_event": 0,
        "ts_init": 0,
    }
