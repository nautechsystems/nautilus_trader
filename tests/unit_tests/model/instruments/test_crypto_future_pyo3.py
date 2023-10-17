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


from nautilus_trader.core.nautilus_pyo3.model import CryptoFuture
from nautilus_trader.test_kit.rust.instruments import TestInstrumentProviderPyo3


crypto_future_btcusdt = TestInstrumentProviderPyo3.btcusdt_future_binance()


class TestCryptoFuture:
    def test_equality(self):
        item_1 = TestInstrumentProviderPyo3.btcusdt_future_binance()
        item_2 = TestInstrumentProviderPyo3.btcusdt_future_binance()
        assert item_1 == item_2

    def test_hash(self):
        assert hash(crypto_future_btcusdt) == hash(crypto_future_btcusdt)

    def test_to_dict(self):
        dict = crypto_future_btcusdt.to_dict()
        assert CryptoFuture.from_dict(dict) == crypto_future_btcusdt
        assert dict == {
            "type": "CryptoPerpetual",
            "id": "BTCUSDT_220325.BINANCE",
            "raw_symbol": "BTCUSDT",
            "underlying": "BTC",
            "quote_currency": "USDT",
            "settlement_currency": "USDT",
            "expiration": 1648162800000000000,
            "price_precision": 2,
            "size_precision": 6,
            "price_increment": "0.01",
            "size_increment": "0.000001",
            "margin_maint": 0.0,
            "margin_init": 0.0,
            "maker_fee": 0.0,
            "taker_fee": 0.0,
            "lot_size": None,
            "max_notional": None,
            "max_price": "1000000.0",
            "max_quantity": "9000",
            "min_notional": "10.00000000 USDT",
            "min_price": "0.01",
            "min_quantity": "0.00001",
        }
