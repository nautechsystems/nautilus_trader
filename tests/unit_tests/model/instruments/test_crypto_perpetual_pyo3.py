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

from nautilus_trader.core.nautilus_pyo3 import CryptoPerpetual
from nautilus_trader.test_kit.rust.instruments_pyo3 import TestInstrumentProviderPyo3


_ETHUSDT_PERP = TestInstrumentProviderPyo3.ethusdt_perp_binance()


def test_equality():
    item_1 = TestInstrumentProviderPyo3.ethusdt_perp_binance()
    item_2 = TestInstrumentProviderPyo3.ethusdt_perp_binance()
    assert item_1 == item_2


def test_hash():
    assert hash(_ETHUSDT_PERP) == hash(_ETHUSDT_PERP)


def test_to_dict():
    result = _ETHUSDT_PERP.to_dict()
    assert CryptoPerpetual.from_dict(result) == _ETHUSDT_PERP
    assert result == {
        "type": "CryptoPerpetual",
        "id": "ETHUSDT-PERP.BINANCE",
        "raw_symbol": "ETHUSDT",
        "base_currency": "ETH",
        "quote_currency": "USDT",
        "settlement_currency": "USDT",
        "is_inverse": False,
        "price_precision": 2,
        "size_precision": 0,
        "price_increment": "0.01",
        "size_increment": "0.001",
        "lot_size": None,
        "max_quantity": "10000",
        "min_quantity": "0.001",
        "max_notional": None,
        "min_notional": "10.00000000 USDT",
        "max_price": "15000.0",
        "min_price": "1.0",
        "ts_event": 0,
        "ts_init": 0,
    }
