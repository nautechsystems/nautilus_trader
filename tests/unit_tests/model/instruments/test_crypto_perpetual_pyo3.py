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

from nautilus_trader.core.nautilus_pyo3 import CryptoPerpetual
from nautilus_trader.test_kit.rust.instruments import TestInstrumentProviderPyo3


crypto_perpetual_ethusdt_perp = TestInstrumentProviderPyo3.ethusdt_perp_binance()


class TestCryptoPerpetual:
    def test_equality(self):
        item_1 = TestInstrumentProviderPyo3.ethusdt_perp_binance()
        item_2 = TestInstrumentProviderPyo3.ethusdt_perp_binance()
        assert item_1 == item_2

    def test_hash(self):
        assert hash(crypto_perpetual_ethusdt_perp) == hash(crypto_perpetual_ethusdt_perp)

    def test_to_dict(self):
        dict = crypto_perpetual_ethusdt_perp.to_dict()
        assert CryptoPerpetual.from_dict(dict) == crypto_perpetual_ethusdt_perp
        assert dict == {
            "type": "CryptoPerpetual",
            "id": "ETHUSDT-PERP.BINANCE",
            "raw_symbol": "ETHUSDT",
            "base_currency": "ETH",
            "quote_currency": "USDT",
            "settlement_currency": "USDT",
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
            "margin_maint": 0.0,
            "margin_init": 0.0,
            "maker_fee": 0.0,
            "taker_fee": 0.0,
        }
