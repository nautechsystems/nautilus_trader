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
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_AAPL_OPTION = TestInstrumentProviderPyo3.aapl_option()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.aapl_option()
    item_2 = TestInstrumentProviderPyo3.aapl_option()
    assert item_1 == item_2


def test_hash():
    assert hash(_AAPL_OPTION) == hash(_AAPL_OPTION)


def test_to_dict():
    result = _AAPL_OPTION.to_dict()
    assert nautilus_pyo3.OptionContract.from_dict(result) == _AAPL_OPTION
    assert result == {
        "type": "OptionContract",
        "id": "AAPL211217C00150000.OPRA",
        "raw_symbol": "AAPL211217C00150000",
        "asset_class": "EQUITY",
        "exchange": "GMNI",
        "underlying": "AAPL",
        "option_kind": "CALL",
        "activation_ns": 1631836800000000000,
        "expiration_ns": 1639699200000000000,
        "strike_price": "149.0",
        "currency": "USDT",
        "price_precision": 2,
        "price_increment": "0.01",
        "size_increment": "1",
        "size_precision": 0,
        "multiplier": "1",
        "lot_size": "1",
        "max_quantity": None,
        "min_quantity": "1",
        "max_price": None,
        "min_price": None,
        "margin_init": "0",
        "margin_maint": "0",
        "maker_fee": "0",
        "taker_fee": "0",
        "info": {},
        "ts_event": 0,
        "ts_init": 0,
    }


def test_legacy_option_contract_from_pyo3():
    option = OptionContract.from_pyo3(_AAPL_OPTION)

    assert option.id.value == "AAPL211217C00150000.OPRA"


def test_pyo3_cython_conversion():
    option_contract_pyo3 = TestInstrumentProviderPyo3.aapl_option()
    option_contract_pyo3_dict = option_contract_pyo3.to_dict()
    option_contract_cython = OptionContract.from_pyo3(option_contract_pyo3)
    option_contract_cython_dict = OptionContract.to_dict(option_contract_cython)
    option_contract_pyo3_back = nautilus_pyo3.OptionContract.from_dict(
        option_contract_cython_dict,
    )
    assert option_contract_cython_dict == option_contract_pyo3_dict
    assert option_contract_pyo3 == option_contract_pyo3_back
