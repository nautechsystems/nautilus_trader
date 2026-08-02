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

from typing import Any
from typing import cast

import pytest

from nautilus_trader.adapters.binance import factories as binance_factories
from nautilus_trader.adapters.binance.common.constants import BINANCE_FUTURES_ORDER_COUNT_1M_KEY
from nautilus_trader.adapters.binance.common.constants import BINANCE_FUTURES_ORDER_COUNT_10S_KEY
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.futures.http.account import BinanceFuturesAlgoOrderHttp
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


class RecordingBinanceHttpClient:
    def __init__(self) -> None:
        self.requests: list[dict[str, Any]] = []

    async def sign_request(
        self,
        http_method: HttpMethod,
        url_path: str,
        payload: dict[str, str] | None = None,
        ratelimiter_keys: list[str] | None = None,
    ) -> bytes:
        self.requests.append(
            {
                "http_method": http_method,
                "url_path": url_path,
                "payload": payload,
                "ratelimiter_keys": ratelimiter_keys,
            },
        )
        return (
            b'{"algoId":2146760,"clientAlgoId":"client-1","algoType":"CONDITIONAL",'
            b'"orderType":"TAKE_PROFIT","symbol":"BNBUSDT","side":"SELL"}'
        )

    async def send_request(
        self,
        http_method: HttpMethod,
        url_path: str,
        payload: dict[str, str] | None = None,
        ratelimiter_keys: list[str] | None = None,
    ) -> bytes:
        raise AssertionError("algo order POST must be signed")


def test_futures_http_client_factory_registers_order_count_limiters(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, Any] = {}

    class CapturingBinanceHttpClient:
        def __init__(self, **kwargs: Any) -> None:
            captured.update(kwargs)

    monkeypatch.setattr(binance_factories, "BinanceHttpClient", CapturingBinanceHttpClient)
    binance_factories.get_cached_binance_http_client.cache_clear()

    try:
        binance_factories.get_cached_binance_http_client(
            clock=LiveClock(),
            account_type=BinanceAccountType.USDT_FUTURES,
            api_key="api-key",
            api_secret="api-secret",
        )
    finally:
        binance_factories.get_cached_binance_http_client.cache_clear()

    quota_keys = [key for key, _ in captured["ratelimiter_quotas"]]
    assert BINANCE_FUTURES_ORDER_COUNT_10S_KEY in quota_keys
    assert BINANCE_FUTURES_ORDER_COUNT_1M_KEY in quota_keys


@pytest.mark.asyncio
async def test_futures_algo_order_post_uses_order_count_limiters() -> None:
    client = RecordingBinanceHttpClient()
    endpoint = BinanceFuturesAlgoOrderHttp(cast(BinanceHttpClient, client), "/fapi/v1/")

    await endpoint.post(
        endpoint.PostParameters(
            symbol=BinanceSymbol("BNBUSDT"),
            side=BinanceOrderSide.SELL,
            type=BinanceOrderType.TAKE_PROFIT,
            algoType="CONDITIONAL",
            timestamp="1760000000000",
        ),
    )

    request = client.requests[0]
    assert request["http_method"] == HttpMethod.POST
    assert request["url_path"] == "/fapi/v1/algoOrder"
    assert request["ratelimiter_keys"] == [
        BINANCE_FUTURES_ORDER_COUNT_10S_KEY,
        BINANCE_FUTURES_ORDER_COUNT_1M_KEY,
        "binance:/fapi/v1/algoOrder",
        "binance:global",
    ]


@pytest.mark.asyncio
async def test_coin_futures_algo_order_post_keeps_default_limiters() -> None:
    client = RecordingBinanceHttpClient()
    endpoint = BinanceFuturesAlgoOrderHttp(cast(BinanceHttpClient, client), "/dapi/v1/")

    await endpoint.post(
        endpoint.PostParameters(
            symbol=BinanceSymbol("BNBUSD_PERP"),
            side=BinanceOrderSide.SELL,
            type=BinanceOrderType.TAKE_PROFIT,
            algoType="CONDITIONAL",
            timestamp="1760000000000",
        ),
    )

    request = client.requests[0]
    assert request["http_method"] == HttpMethod.POST
    assert request["url_path"] == "/dapi/v1/algoOrder"
    assert request["ratelimiter_keys"] == [
        "binance:/dapi/v1/algoOrder",
        "binance:global",
    ]
