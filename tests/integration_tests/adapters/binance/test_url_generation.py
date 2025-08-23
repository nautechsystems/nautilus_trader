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

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.urls import get_http_base_url
from nautilus_trader.adapters.binance.common.urls import get_ws_base_url


@pytest.mark.parametrize(
    ("account_type", "is_testnet", "is_us", "expected"),
    [
        # Live URLs
        (BinanceAccountType.SPOT, False, False, "https://api.binance.com"),
        (BinanceAccountType.SPOT, False, True, "https://api.binance.us"),
        (BinanceAccountType.MARGIN, False, False, "https://sapi.binance.com"),
        (BinanceAccountType.MARGIN, False, True, "https://sapi.binance.us"),
        (BinanceAccountType.USDT_FUTURES, False, False, "https://fapi.binance.com"),
        (BinanceAccountType.USDT_FUTURES, False, True, "https://fapi.binance.us"),
        (BinanceAccountType.COIN_FUTURES, False, False, "https://dapi.binance.com"),
        (BinanceAccountType.COIN_FUTURES, False, True, "https://dapi.binance.us"),
        # Testnet URLs (US flag ignored)
        (BinanceAccountType.SPOT, True, False, "https://testnet.binance.vision"),
        (BinanceAccountType.SPOT, True, True, "https://testnet.binance.vision"),
        (BinanceAccountType.MARGIN, True, False, "https://testnet.binance.vision"),
        (BinanceAccountType.USDT_FUTURES, True, False, "https://testnet.binancefuture.com"),
        (BinanceAccountType.COIN_FUTURES, True, False, "https://testnet.binancefuture.com"),
    ],
)
def test_get_http_base_url(account_type, is_testnet, is_us, expected):
    url = get_http_base_url(account_type, is_testnet=is_testnet, is_us=is_us)
    assert url == expected


@pytest.mark.parametrize(
    ("account_type", "is_testnet", "is_us", "expected"),
    [
        # Live URLs
        (BinanceAccountType.SPOT, False, False, "wss://stream.binance.com:9443"),
        (BinanceAccountType.SPOT, False, True, "wss://stream.binance.us:9443"),
        (BinanceAccountType.MARGIN, False, False, "wss://stream.binance.com:9443"),
        (BinanceAccountType.MARGIN, False, True, "wss://stream.binance.us:9443"),
        (BinanceAccountType.USDT_FUTURES, False, False, "wss://fstream.binance.com"),
        (BinanceAccountType.USDT_FUTURES, False, True, "wss://fstream.binance.us"),
        (BinanceAccountType.COIN_FUTURES, False, False, "wss://dstream.binance.com"),
        (BinanceAccountType.COIN_FUTURES, False, True, "wss://dstream.binance.us"),
        # Testnet URLs (US flag ignored)
        (BinanceAccountType.SPOT, True, False, "wss://stream.testnet.binance.vision"),
        (BinanceAccountType.SPOT, True, True, "wss://stream.testnet.binance.vision"),
        (BinanceAccountType.MARGIN, True, False, "wss://stream.testnet.binance.vision"),
        (BinanceAccountType.USDT_FUTURES, True, False, "wss://stream.binancefuture.com"),
    ],
)
def test_get_ws_base_url(account_type, is_testnet, is_us, expected):
    url = get_ws_base_url(account_type, is_testnet=is_testnet, is_us=is_us)
    assert url == expected


def test_get_ws_base_url_coin_futures_testnet_raises_error():
    with pytest.raises(ValueError, match="no testnet for COIN-M futures"):
        get_ws_base_url(BinanceAccountType.COIN_FUTURES, is_testnet=True, is_us=False)
