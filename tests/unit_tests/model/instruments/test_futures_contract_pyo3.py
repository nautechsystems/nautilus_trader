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

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_ES_FUTURE = TestInstrumentProviderPyo3.futures_contract_es()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.futures_contract_es()
    item_2 = TestInstrumentProviderPyo3.futures_contract_es()
    assert item_1 == item_2


def test_hash():
    assert hash(_ES_FUTURE) == hash(_ES_FUTURE)


def test_to_dict():
    result = _ES_FUTURE.to_dict()
    assert nautilus_pyo3.FuturesContract.from_dict(result) == _ES_FUTURE
    assert result == {
        "type": "FuturesContract",
        "id": "ESZ21.GLBX",
        "raw_symbol": "ESZ21",
        "asset_class": "INDEX",
        "underlying": "ES",
        "activation_ns": 1631836800000000000,
        "expiration_ns": 1639699200000000000,
        "currency": "USD",
        "price_precision": 2,
        "price_increment": "0.01",
        "size_precision": 0,
        "size_increment": "1",
        "multiplier": "1",
        "lot_size": "1",
        "max_price": None,
        "max_quantity": None,
        "min_price": None,
        "min_quantity": "1",
        "margin_init": "0",
        "margin_maint": "0",
        "maker_fee": "0",
        "taker_fee": "0",
        "info": {},
        "ts_event": 0,
        "ts_init": 0,
        "exchange": "XCME",
    }


def test_legacy_futures_contract_from_pyo3():
    future = FuturesContract.from_pyo3(_ES_FUTURE)

    assert future.id.value == "ESZ21.GLBX"


def test_pyo3_cython_conversion():
    futures_contract_pyo3 = TestInstrumentProviderPyo3.futures_contract_es()
    futures_contract_pyo3_dict = futures_contract_pyo3.to_dict()
    futures_contract_cython = FuturesContract.from_pyo3(futures_contract_pyo3)
    futures_contract_cython_dict = FuturesContract.to_dict(futures_contract_cython)
    futures_contract_pyo3_back = nautilus_pyo3.FuturesContract.from_dict(
        futures_contract_cython_dict,
    )
    assert futures_contract_pyo3 == futures_contract_pyo3_back
    assert futures_contract_pyo3_dict == futures_contract_cython_dict
