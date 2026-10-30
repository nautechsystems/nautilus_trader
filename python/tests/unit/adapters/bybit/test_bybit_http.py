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
Tests Bybit HTTP amendment fields through the Python client.
"""

import json
from collections.abc import Iterator
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
from pathlib import Path
from threading import Thread

import pytest

from nautilus_trader.adapters.bybit import BybitHttpClient
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.model import AccountId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import VenueOrderId


@pytest.fixture
def amend_requests() -> Iterator[tuple[BybitHttpClient, list[dict[str, object]]]]:
    """
    Capture amendments sent through the Python HTTP client.
    """
    requests = []
    instruments = (
        Path(__file__).resolve().parents[5]
        / "crates/adapters/bybit/test_data/http_get_instruments_linear.json"
    ).read_bytes()

    class AmendRequestHandler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(instruments)))
            self.end_headers()
            self.wfile.write(instruments)

        def do_POST(self) -> None:
            requests.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
            body = json.dumps(
                {
                    "retCode": 10001,
                    "retMsg": "captured amendment",
                    "result": {},
                    "time": 1704470400123,
                },
            ).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, _format: str, *_args: object) -> None:
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), AmendRequestHandler)
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    client = BybitHttpClient(
        api_key="test_key",
        api_secret="test_secret",
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
    ("quantity", "price", "trigger_price"),
    [
        pytest.param(None, None, "1732.45", id="trigger-only"),
        pytest.param("0.023", "1741.23", "1732.45", id="combined"),
        pytest.param("0.023", "1741.23", None, id="limit"),
    ],
)
@pytest.mark.asyncio
async def test_modify_order_forwards_optional_trigger_price(
    amend_requests,
    quantity: str | None,
    price: str | None,
    trigger_price: str | None,
) -> None:
    """
    Preserve supplied amendment values and omit absent fields.
    """
    client, requests = amend_requests
    await client.request_instruments(BybitProductType.LINEAR)
    kwargs = {}
    expected = {"category": "linear", "symbol": "ETHUSDT", "orderId": "test-order-1"}

    if quantity is not None:
        kwargs["quantity"] = Quantity.from_str(quantity)
        expected["qty"] = quantity
    if price is not None:
        kwargs["price"] = Price.from_str(price)
        expected["price"] = price
    if trigger_price is not None:
        kwargs["trigger_price"] = Price.from_str(trigger_price)
        expected["triggerPrice"] = trigger_price

    with pytest.raises(ValueError, match="Bybit error 10001: captured amendment"):
        await client.modify_order(
            AccountId("BYBIT-001"),
            BybitProductType.LINEAR,
            InstrumentId.from_str("ETHUSDT-LINEAR.BYBIT"),
            venue_order_id=VenueOrderId("test-order-1"),
            **kwargs,
        )

    assert requests == [expected]
