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
from nautilus_trader.model.instruments import CryptoOption
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_BTCUSD_OPTION = TestInstrumentProviderPyo3.btcusd_option_deribit()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.btcusd_option_deribit()
    item_2 = TestInstrumentProviderPyo3.btcusd_option_deribit()
    assert item_1 == item_2


def test_hash():
    assert hash(_BTCUSD_OPTION) == hash(_BTCUSD_OPTION)


def test_to_dict():
    result = _BTCUSD_OPTION.to_dict()
    assert nautilus_pyo3.CryptoOption.from_dict(result) == _BTCUSD_OPTION
    assert result == {
        "type": "CryptoOption",
        "id": "BTC-13JAN23-16000-P.DERIBIT",
        "raw_symbol": "BTC-13JAN23-16000-P",
        "underlying": "BTC",
        "quote_currency": "USD",
        "settlement_currency": "BTC",
        "is_inverse": False,
        "option_kind": "PUT",
        "strike_price": "16000.00",
        "activation_ns": 1671696002000000000,
        "expiration_ns": 1673596800000000000,
        "price_precision": 2,
        "size_precision": 1,
        "price_increment": "0.01",
        "size_increment": "0.1",
        "multiplier": "1",
        "lot_size": "1",
        "margin_init": "0",
        "margin_maint": "0",
        "maker_fee": "0.0003",
        "taker_fee": "0.0003",
        "ts_event": 0,
        "ts_init": 0,
        "info": {},
        "max_quantity": "9000",
        "min_quantity": "0.1",
        "max_notional": None,
        "min_notional": "10.00 USD",
        "max_price": None,
        "min_price": None,
    }


def test_pyo3_cython_conversion():
    crypto_option_pyo3 = TestInstrumentProviderPyo3.btcusd_option_deribit()
    crypto_option_pyo3_dict = crypto_option_pyo3.to_dict()
    crypto_option_cython = CryptoOption.from_pyo3(crypto_option_pyo3)
    crypto_option_cython_dict = CryptoOption.to_dict(crypto_option_cython)
    crypto_option_pyo3_back = nautilus_pyo3.CryptoOption.from_dict(crypto_option_cython_dict)
    assert crypto_option_pyo3 == crypto_option_pyo3_back
    assert crypto_option_pyo3_dict == crypto_option_cython_dict
