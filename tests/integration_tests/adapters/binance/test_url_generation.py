# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.binance.common.urls import get_http_base_url
from nautilus_trader.adapters.binance.common.urls import get_usdm_ws_route_base_url
from nautilus_trader.adapters.binance.common.urls import get_ws_api_base_url
from nautilus_trader.adapters.binance.common.urls import get_ws_base_url
from nautilus_trader.adapters.binance.common.urls import get_ws_private_base_url
from nautilus_trader.adapters.binance.common.urls import get_ws_public_base_url


@pytest.mark.parametrize(
    ("account_type", "environment", "is_us", "expected"),
    [
        # Live URLs
        (BinanceAccountType.SPOT, BinanceEnvironment.LIVE, False, "https://api.binance.com"),
        (BinanceAccountType.SPOT, BinanceEnvironment.LIVE, True, "https://api.binance.us"),
        (BinanceAccountType.MARGIN, BinanceEnvironment.LIVE, False, "https://sapi.binance.com"),
        (BinanceAccountType.MARGIN, BinanceEnvironment.LIVE, True, "https://sapi.binance.us"),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.LIVE,
            False,
            "https://fapi.binance.com",
        ),
        (BinanceAccountType.USDT_FUTURES, BinanceEnvironment.LIVE, True, "https://fapi.binance.us"),
        (
            BinanceAccountType.COIN_FUTURES,
            BinanceEnvironment.LIVE,
            False,
            "https://dapi.binance.com",
        ),
        (BinanceAccountType.COIN_FUTURES, BinanceEnvironment.LIVE, True, "https://dapi.binance.us"),
        # Testnet URLs
        (
            BinanceAccountType.SPOT,
            BinanceEnvironment.TESTNET,
            False,
            "https://testnet.binance.vision",
        ),
        (
            BinanceAccountType.MARGIN,
            BinanceEnvironment.TESTNET,
            False,
            "https://testnet.binance.vision",
        ),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.TESTNET,
            False,
            "https://testnet.binancefuture.com",
        ),
        (
            BinanceAccountType.COIN_FUTURES,
            BinanceEnvironment.TESTNET,
            False,
            "https://testnet.binancefuture.com",
        ),
        # Demo URLs
        (BinanceAccountType.SPOT, BinanceEnvironment.DEMO, False, "https://demo-api.binance.com"),
        (BinanceAccountType.MARGIN, BinanceEnvironment.DEMO, False, "https://demo-api.binance.com"),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.DEMO,
            False,
            "https://demo-fapi.binance.com",
        ),
        (
            BinanceAccountType.COIN_FUTURES,
            BinanceEnvironment.DEMO,
            False,
            "https://testnet.binancefuture.com",
        ),
    ],
)
def test_get_http_base_url(account_type, environment, is_us, expected):
    url = get_http_base_url(account_type, environment=environment, is_us=is_us)
    assert url == expected


@pytest.mark.parametrize(
    ("account_type", "environment", "is_us", "expected"),
    [
        # Live URLs
        (BinanceAccountType.SPOT, BinanceEnvironment.LIVE, False, "wss://stream.binance.com:9443"),
        (BinanceAccountType.SPOT, BinanceEnvironment.LIVE, True, "wss://stream.binance.us:9443"),
        (
            BinanceAccountType.MARGIN,
            BinanceEnvironment.LIVE,
            False,
            "wss://stream.binance.com:9443",
        ),
        (BinanceAccountType.MARGIN, BinanceEnvironment.LIVE, True, "wss://stream.binance.us:9443"),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.LIVE,
            False,
            "wss://fstream.binance.com/market",
        ),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.LIVE,
            True,
            "wss://fstream.binance.us/market",
        ),
        (
            BinanceAccountType.COIN_FUTURES,
            BinanceEnvironment.LIVE,
            False,
            "wss://dstream.binance.com",
        ),
        (
            BinanceAccountType.COIN_FUTURES,
            BinanceEnvironment.LIVE,
            True,
            "wss://dstream.binance.us",
        ),
        # Testnet URLs
        (
            BinanceAccountType.SPOT,
            BinanceEnvironment.TESTNET,
            False,
            "wss://stream.testnet.binance.vision",
        ),
        (
            BinanceAccountType.MARGIN,
            BinanceEnvironment.TESTNET,
            False,
            "wss://stream.testnet.binance.vision",
        ),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.TESTNET,
            False,
            "wss://stream.binancefuture.com",
        ),
        (
            BinanceAccountType.COIN_FUTURES,
            BinanceEnvironment.TESTNET,
            False,
            "wss://dstream.binancefuture.com",
        ),
        # Demo URLs
        (BinanceAccountType.SPOT, BinanceEnvironment.DEMO, False, "wss://demo-stream.binance.com"),
        (
            BinanceAccountType.MARGIN,
            BinanceEnvironment.DEMO,
            False,
            "wss://demo-stream.binance.com",
        ),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.DEMO,
            False,
            "wss://stream.binancefuture.com",
        ),
        (
            BinanceAccountType.COIN_FUTURES,
            BinanceEnvironment.DEMO,
            False,
            "wss://dstream.binancefuture.com",
        ),
    ],
)
def test_get_ws_base_url(account_type, environment, is_us, expected):
    url = get_ws_base_url(account_type, environment=environment, is_us=is_us)
    assert url == expected


@pytest.mark.parametrize(
    ("account_type", "environment", "is_us", "expected"),
    [
        # Live URLs
        (
            BinanceAccountType.SPOT,
            BinanceEnvironment.LIVE,
            False,
            "wss://ws-api.binance.com:443/ws-api/v3",
        ),
        (
            BinanceAccountType.SPOT,
            BinanceEnvironment.LIVE,
            True,
            "wss://ws-api.binance.us:443/ws-api/v3",
        ),
        (
            BinanceAccountType.MARGIN,
            BinanceEnvironment.LIVE,
            False,
            "wss://ws-api.binance.com:443/ws-api/v3",
        ),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.LIVE,
            False,
            "wss://ws-fapi.binance.com/ws-fapi/v1",
        ),
        (
            BinanceAccountType.COIN_FUTURES,
            BinanceEnvironment.LIVE,
            False,
            "wss://ws-dapi.binance.com/ws-dapi/v1",
        ),
        # Testnet URLs
        (
            BinanceAccountType.SPOT,
            BinanceEnvironment.TESTNET,
            False,
            "wss://ws-api.testnet.binance.vision/ws-api/v3",
        ),
        (
            BinanceAccountType.MARGIN,
            BinanceEnvironment.TESTNET,
            False,
            "wss://ws-api.testnet.binance.vision/ws-api/v3",
        ),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.TESTNET,
            False,
            "wss://testnet.binancefuture.com/ws-fapi/v1",
        ),
        # Demo URLs
        (
            BinanceAccountType.SPOT,
            BinanceEnvironment.DEMO,
            False,
            "wss://demo-ws-api.binance.com/ws-api/v3",
        ),
        (
            BinanceAccountType.MARGIN,
            BinanceEnvironment.DEMO,
            False,
            "wss://demo-ws-api.binance.com/ws-api/v3",
        ),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.DEMO,
            False,
            "wss://testnet.binancefuture.com/ws-fapi/v1",
        ),
    ],
)
def test_get_ws_api_base_url(account_type, environment, is_us, expected):
    url = get_ws_api_base_url(account_type, environment=environment, is_us=is_us)
    assert url == expected


@pytest.mark.parametrize(
    ("account_type", "environment"),
    [
        (BinanceAccountType.COIN_FUTURES, BinanceEnvironment.TESTNET),
        (BinanceAccountType.COIN_FUTURES, BinanceEnvironment.DEMO),
    ],
)
def test_get_ws_api_base_url_raises_for_coin_futures(account_type, environment):
    with pytest.raises(ValueError):
        get_ws_api_base_url(account_type, environment=environment, is_us=False)


@pytest.mark.parametrize(
    ("account_type", "environment", "is_us", "expected"),
    [
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.LIVE,
            False,
            "wss://fstream.binance.com/private",
        ),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.LIVE,
            True,
            "wss://fstream.binance.us/private",
        ),
        (
            BinanceAccountType.COIN_FUTURES,
            BinanceEnvironment.LIVE,
            False,
            "wss://dstream.binance.com",
        ),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.TESTNET,
            False,
            "wss://stream.binancefuture.com",
        ),
    ],
)
def test_get_ws_private_base_url(account_type, environment, is_us, expected):
    url = get_ws_private_base_url(account_type, environment=environment, is_us=is_us)
    assert url == expected


@pytest.mark.parametrize(
    ("account_type", "environment", "is_us", "expected"),
    [
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.LIVE,
            False,
            "wss://fstream.binance.com/public",
        ),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.LIVE,
            True,
            "wss://fstream.binance.us/public",
        ),
        (
            BinanceAccountType.COIN_FUTURES,
            BinanceEnvironment.LIVE,
            False,
            "wss://dstream.binance.com",
        ),
        (
            BinanceAccountType.USDT_FUTURES,
            BinanceEnvironment.TESTNET,
            False,
            "wss://stream.binancefuture.com",
        ),
    ],
)
def test_get_ws_public_base_url(account_type, environment, is_us, expected):
    url = get_ws_public_base_url(account_type, environment=environment, is_us=is_us)
    assert url == expected


@pytest.mark.parametrize(
    ("base_url", "route", "expected"),
    [
        ("wss://fstream.binance.com", "market", "wss://fstream.binance.com/market"),
        ("wss://fstream.binance.com/ws", "public", "wss://fstream.binance.com/public"),
        (
            "wss://fstream.binance.com/market/ws",
            "private",
            "wss://fstream.binance.com/private",
        ),
        (
            "wss://fstream-mm.binance.com",
            "market",
            "wss://fstream-mm.binance.com/market",
        ),
        (
            "wss://fstream-mm.binance.com/ws",
            "public",
            "wss://fstream-mm.binance.com/public",
        ),
        (
            "wss://fstream-auth.binance.com/market/ws",
            "private",
            "wss://fstream-auth.binance.com/private",
        ),
        ("wss://fstream.binance.us", "market", "wss://fstream.binance.us/market"),
    ],
)
def test_get_usdm_ws_route_base_url_normalizes_fstream_override(base_url, route, expected):
    assert get_usdm_ws_route_base_url(base_url, route) == expected


@pytest.mark.parametrize(
    ("base_url", "route"),
    [
        ("ws://127.0.0.1:9999/ws", "market"),
        ("wss://other.example.com/private/ws", "private"),
        ("ws://localhost:8080", "public"),
        ("wss://other-fstream.binance.com.example.org/ws", "market"),
        ("wss://fstream.binance.com.example.org/ws", "market"),
    ],
)
def test_get_usdm_ws_route_base_url_passes_through_non_binance_host(base_url, route):
    assert get_usdm_ws_route_base_url(base_url, route) == base_url


def test_get_usdm_ws_route_base_url_raises_on_invalid_route():
    with pytest.raises(ValueError, match="invalid USD-M WebSocket route"):
        get_usdm_ws_route_base_url("wss://fstream.binance.com", "bogus")
