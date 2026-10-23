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
"""
Tests cash-spot algo quantity denomination.
"""

import json
from collections.abc import Iterator
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
from threading import Thread

import pytest

from nautilus_trader.adapters.okx import OKXHttpClient
from nautilus_trader.adapters.okx import OKXTradeMode
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TraderId


@pytest.fixture
def algo_requests() -> Iterator[tuple[OKXHttpClient, list[dict[str, object]]]]:
    """
    Capture algo requests sent through the native Python client.
    """
    requests = []

    class AlgoRequestHandler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:
            request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            requests.append(request)
            body = json.dumps(
                {
                    "code": "0",
                    "msg": "",
                    "data": [
                        {
                            "algoId": "7003",
                            "algoClOrdId": request["algoClOrdId"],
                            "sCode": "0",
                            "sMsg": "",
                        },
                    ],
                },
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, _format: str, *_args: object) -> None:
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), AlgoRequestHandler)
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    client = OKXHttpClient(
        api_key="test_key",
        api_secret="test_secret",
        api_passphrase="test_passphrase",
        base_url=f"http://127.0.0.1:{server.server_port}",
        max_retries=0,
    )
    try:
        yield client, requests
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


@pytest.mark.parametrize(
    "trade_mode",
    [OKXTradeMode.CASH, OKXTradeMode.CROSS, OKXTradeMode.ISOLATED],
)
@pytest.mark.parametrize("quote_quantity", [None, False, True])
@pytest.mark.parametrize(
    ("order_type", "leg"),
    [
        (OrderType.STOP_MARKET, "sl"),
        (OrderType.MARKET_IF_TOUCHED, "tp"),
    ],
)
@pytest.mark.asyncio
async def test_place_algo_order_preserves_spot_buy_denomination(
    algo_requests,
    quote_quantity,
    order_type,
    leg,
    trade_mode,
) -> None:
    """
    Preserves default base sizing and explicit denomination for both buy types.
    """
    client, requests = algo_requests
    kwargs = {} if quote_quantity is None else {"quote_quantity": quote_quantity}
    response = await client.place_algo_order(
        TraderId("TESTER-001"),
        StrategyId("TEST-001"),
        InstrumentId.from_str("BTC-USDT.OKX"),
        trade_mode,
        ClientOrderId("OSPOTPY"),
        OrderSide.BUY,
        order_type,
        Quantity.from_str("0.01234567"),
        Price.from_str("51234.5"),
        **kwargs,
    )

    assert response == {"algo_id": "7003", "algo_cl_ord_id": "OSPOTPY", "s_code": "0", "s_msg": ""}
    assert requests == [
        {
            "instId": "BTC-USDT",
            "tdMode": str(trade_mode).lower(),
            "side": "buy",
            "ordType": "conditional",
            "algoClOrdId": "OSPOTPY",
            "sz": "0.01234567",
            "tag": "5328c82e5542BCDE",
            "tgtCcy": "quote_ccy" if quote_quantity else "base_ccy",
            f"{leg}TriggerPx": "51234.5",
            f"{leg}OrdPx": "-1",
            f"{leg}TriggerPxType": "last",
            **({"ccy": "USDT"} if trade_mode != OKXTradeMode.CASH else {}),
        },
    ]


@pytest.mark.asyncio
async def test_place_algo_order_rejects_spot_quote_sell(algo_requests) -> None:
    """
    Rejects unsupported quote sells before sending an HTTP request.
    """
    client, requests = algo_requests
    with pytest.raises(
        ValueError,
        match="Spot STOP_MARKET algo orders do not support quote quantity in Cash mode",
    ):
        await client.place_algo_order(
            TraderId("TESTER-001"),
            StrategyId("TEST-001"),
            InstrumentId.from_str("BTC-USDT.OKX"),
            OKXTradeMode.CASH,
            ClientOrderId("OSPOTPY"),
            OrderSide.SELL,
            OrderType.STOP_MARKET,
            Quantity.from_str("12.34"),
            Price.from_str("51234.5"),
            quote_quantity=True,
        )

    assert requests == []
