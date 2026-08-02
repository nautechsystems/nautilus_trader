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

import json
from collections.abc import Iterator
from datetime import UTC
from datetime import datetime
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
from threading import Thread
from types import SimpleNamespace
from urllib.parse import parse_qs
from urllib.parse import urlparse

import pytest

from nautilus_trader.adapters.polymarket import PolymarketDataLoader
from nautilus_trader.model import BinaryOption


CONDITION_ID = "0xcondition"
YES_TOKEN = "yes-token"
NO_TOKEN = "no-token"


def _gamma_market(slug: str = "test-market") -> dict[str, object]:
    return {
        "id": "100001",
        "conditionId": CONDITION_ID,
        "questionID": "0xquestion",
        "clobTokenIds": "[]",
        "outcomes": "[]",
        "question": "Will the loader test pass?",
        "description": "Test market",
        "startDate": "2026-01-01T00:00:00Z",
        "endDate": "2026-12-31T00:00:00Z",
        "active": False,
        "closed": True,
        "closedTime": "2026-06-01T00:00:00Z",
        "umaResolutionStatus": "resolved",
        "resolutionSource": "https://example.com/result",
        "acceptingOrders": False,
        "enableOrderBook": True,
        "orderPriceMinTickSize": 0.01,
        "slug": slug,
        "negRisk": False,
        "events": [],
    }


def _clob_market(slug: str) -> dict[str, object]:
    tokens: list[dict[str, object]] = [
        {"token_id": YES_TOKEN, "outcome": "Yes", "winner": True},
        {"token_id": NO_TOKEN, "outcome": "No", "winner": False},
    ]

    if slug == "empty-tokens":
        tokens.clear()
    elif slug == "empty-id":
        tokens[0]["token_id"] = ""
    elif slug == "malformed-market":
        tokens.pop()

    return {
        "condition_id": CONDITION_ID,
        "closed": True,
        "tokens": tokens,
    }


def _trade(asset: str, timestamp: int, suffix: str) -> dict[str, object]:
    return {
        "asset": asset,
        "conditionId": CONDITION_ID,
        "side": "BUY",
        "price": 0.60,
        "size": 10.0,
        "timestamp": timestamp,
        "transactionHash": f"0x{suffix}",
    }


class _PolymarketRequestHandler(BaseHTTPRequestHandler):
    query_log: list[tuple[str, dict[str, list[str]]]] = []
    current_slug = "test-market"

    def do_GET(self) -> None:
        parsed = urlparse(self.path)
        query = parse_qs(parsed.query)
        self.query_log.append((parsed.path, query))

        if parsed.path.startswith("/markets/slug/"):
            self._send_market_by_slug(parsed.path)
        elif parsed.path == "/markets/keyset":
            self._send_json(self._markets(query))
        elif parsed.path == "/events/keyset":
            self._send_json({"events": [self._event()], "next_cursor": None})
        elif parsed.path == "/events":
            slug = query.get("slug", [""])[0]
            self._send_json([] if slug == "missing-event" else [self._event(slug)])
        elif parsed.path == "/tags":
            self._send_json([{"id": "1", "label": "Test", "slug": "test"}])
        elif parsed.path == "/public-search":
            self._send_json({"markets": [_gamma_market()], "events": [self._event()]})
        elif parsed.path == "/trades":
            self._send_json(
                [
                    _trade(YES_TOKEN, 1_710_000_000, "a"),
                    _trade(NO_TOKEN, 1_710_000_001, "b"),
                    _trade(YES_TOKEN, 1_710_000_002, "c"),
                    _trade(YES_TOKEN, 1_710_000_003, "d"),
                    _trade(YES_TOKEN, 1_710_000_004, "e"),
                    _trade(YES_TOKEN, 1_710_000_005, "f"),
                ],
            )
        elif parsed.path.startswith("/markets/"):
            self._send_json(_clob_market(self.current_slug))
        else:
            self.send_error(404)

    def _send_market_by_slug(self, path: str) -> None:
        slug = path.removeprefix("/markets/slug/")
        type(self).current_slug = slug
        if slug == "missing-market":
            self.send_error(404)
        elif slug == "malformed-response":
            self._send_json({"id": "broken"})
        else:
            self._send_json(_gamma_market(slug))

    def log_message(self, _format: str, *args: object) -> None:
        return

    def _markets(self, query: dict[str, list[str]]) -> dict[str, object]:
        slug = query.get("slug", ["test-market"])[0]
        if "slug" in query:
            type(self).current_slug = slug
        if slug == "missing-market":
            markets: list[dict[str, object]] = []
        elif slug == "malformed-response":
            markets = [{"id": "broken"}]
        else:
            market = _gamma_market(slug)
            if "condition_ids" in query:
                market["feeSchedule"] = {
                    "exponent": 2.0,
                    "rate": 0.02,
                    "takerOnly": True,
                    "rebateRate": 0.0,
                }
            markets = [market]
        return {"markets": markets, "next_cursor": None}

    @staticmethod
    def _event(slug: str = "test-event") -> dict[str, object]:
        return {
            "id": "200001",
            "slug": slug,
            "title": "Test event",
            "active": False,
            "closed": True,
            "markets": [_gamma_market()],
        }

    def _send_json(self, value: object) -> None:
        body = json.dumps(value).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


@pytest.fixture
def polymarket_api() -> Iterator[SimpleNamespace]:
    _PolymarketRequestHandler.query_log = []
    _PolymarketRequestHandler.current_slug = "test-market"
    server = ThreadingHTTPServer(("127.0.0.1", 0), _PolymarketRequestHandler)
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    address = f"http://127.0.0.1:{server.server_port}"

    try:
        yield SimpleNamespace(address=address, query_log=_PolymarketRequestHandler.query_log)
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


@pytest.mark.asyncio
async def test_loader_factory_and_historical_window(polymarket_api: SimpleNamespace) -> None:
    address = polymarket_api.address
    loader = await PolymarketDataLoader.from_market_slug(
        "test-market",
        token_index=0,
        base_url_http=address,
        base_url_gamma=address,
        base_url_data_api=address,
    )

    trades = await loader.load_trades(
        start=datetime.fromtimestamp(1_710_000_002, tz=UTC),
        end=datetime.fromtimestamp(1_710_000_004, tz=UTC),
        limit=2,
    )

    assert isinstance(loader.instrument, BinaryOption)
    assert loader.token_id == YES_TOKEN
    assert loader.condition_id == CONDITION_ID
    assert str(loader.instrument.taker_fee) == "0.02"
    assert [trade.ts_event for trade in trades] == [
        1_710_000_002_000_000_000,
        1_710_000_003_000_000_000,
    ]
    assert loader.resolution_metadata["closed"] is True
    assert loader.resolution_metadata["tokens"][0]["winner"] is True
    assert "closed" not in loader.instrument.info
    assert "winner" not in loader.instrument.info
    assert any("condition_ids" in query for _, query in polymarket_api.query_log)
    assert any(query.get("start") == ["1710000002"] for _, query in polymarket_api.query_log)
    assert any(query.get("end") == ["1710000004"] for _, query in polymarket_api.query_log)


@pytest.mark.asyncio
async def test_loader_discovery_and_event_factory(polymarket_api: SimpleNamespace) -> None:
    address = polymarket_api.address

    market = await PolymarketDataLoader.query_market_by_slug("test-market", address)
    details = await PolymarketDataLoader.query_market_details(CONDITION_ID, address)
    event = await PolymarketDataLoader.query_event_by_slug("test-event", address)
    markets = await PolymarketDataLoader.query_markets({"is_active": False}, address)
    events = await PolymarketDataLoader.query_events({"active": False}, address)
    tags = await PolymarketDataLoader.query_tags(address)
    search = await PolymarketDataLoader.query_search("test", base_url_gamma=address)
    loaders = await PolymarketDataLoader.from_event_slug(
        "test-event",
        token_index=1,
        base_url_http=address,
        base_url_gamma=address,
        base_url_data_api=address,
    )

    assert market["slug"] == "test-market"
    assert details["condition_id"] == CONDITION_ID
    assert event["slug"] == "test-event"
    assert markets[0]["conditionId"] == CONDITION_ID
    assert events[0]["id"] == "200001"
    assert tags == [{"id": "1", "label": "Test", "slug": "test"}]
    assert search["markets"][0]["id"] == "100001"
    assert len(loaders) == 1
    assert loaders[0].token_id == NO_TOKEN
    assert ("/markets/slug/test-market", {}) in polymarket_api.query_log


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("slug", "message"),
    [
        ("missing-market", "Market with slug 'missing-market' not found"),
        ("empty-tokens", "No tokens found"),
        ("empty-id", "has an empty token ID"),
        ("malformed-market", "Expected 2 token IDs"),
    ],
)
async def test_loader_rejects_invalid_market_data(
    polymarket_api: SimpleNamespace,
    slug: str,
    message: str,
) -> None:
    address = polymarket_api.address

    with pytest.raises(ValueError, match=message):
        await PolymarketDataLoader.from_market_slug(
            slug,
            base_url_http=address,
            base_url_gamma=address,
            base_url_data_api=address,
        )


@pytest.mark.asyncio
async def test_loader_rejects_missing_event_and_malformed_response(
    polymarket_api: SimpleNamespace,
) -> None:
    address = polymarket_api.address

    with pytest.raises(ValueError, match="Event with slug 'missing-event' not found"):
        await PolymarketDataLoader.from_event_slug(
            "missing-event",
            base_url_http=address,
            base_url_gamma=address,
            base_url_data_api=address,
        )

    with pytest.raises(RuntimeError, match="missing field"):
        await PolymarketDataLoader.query_market_by_slug("malformed-response", address)


def test_loader_rejects_negative_token_index() -> None:
    with pytest.raises(ValueError, match="Token index -1 cannot be negative"):
        PolymarketDataLoader.from_market_slug("test-market", token_index=-1)
