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
Exercise typed FinFeed REST bindings through a real Python event loop and HTTP peer.
"""

import ast
import asyncio
import json
import runpy
import threading
from collections import deque
from collections.abc import Iterator
from contextlib import contextmanager
from contextlib import suppress
from decimal import Decimal
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
from pathlib import Path
from typing import Any

import pytest
from pandas import Timestamp

from nautilus_trader.adapters import finfeed as ff


TEST_DATA = Path(__file__).resolve().parents[5] / "crates/adapters/finfeed/test_data"
FIELDS = {
    "FinFeedExchange": "exchange_id market_name_institution_description website",
    "FinFeedMarket": "market_id title description outcome_name price status exchange exchange_id outcome_type mechanism source_specific_data",
    "FinFeedMarketsPage": "data next_cursor",
    "FinFeedTrade": "id market_id price quantity timestamp outcome side",
    "FinFeedQuote": "ask bid ask_volume bid_volume entry_time recv_time",
    "FinFeedActivityCurrent": "trade quote",
    "FinFeedActivityBatch": "trades quotes",
    "FinFeedBookLevel": "price size",
    "FinFeedBook": "exchange_id market_id time_exchange time_coinapi asks bids",
    "FinFeedBookEvent": "symbol_id time_exchange time_coinapi price size is_sell_ask update_type id",
    "FinFeedOhlcvPeriod": "period_id length_seconds length_months unit_count unit_name display_name",
    "FinFeedOhlcv": "market_id_exchange time_period_start time_period_end time_open time_close price_open price_high price_low price_close volume_traded trades_count",
}
ENUMS = {
    "status": "FinFeedMarketStatus",
    "outcome_type": "FinFeedOutcomeType",
    "side": "FinFeedTradeSide",
}


@contextmanager
def _peer(responses, **controls: Any) -> Iterator:
    requests = []
    pending = deque(responses)
    entered = threading.Event()
    release = threading.Event()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            requests.append(
                (self.path, self.headers.get("Authorization"), self.headers.get("X-Referrer")),
            )
            entered.set()

            if controls.get("block"):
                release.wait(10)
            status, body = pending.popleft() if pending else (500, b"")
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            with suppress(BrokenPipeError, ConnectionResetError):
                self.wfile.write(body)

        def log_message(self, *_args: object) -> None:
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    config = ff.FinFeedDataClientConfig(
        api_key="python-rest-secret",
        base_url_http=f"http://127.0.0.1:{server.server_port}/v1/",
        min_request_interval_ms=1,
        max_concurrent_requests=1,
        attempt_budget=controls.get("attempt_budget", 30),
        max_retries=0,
        http_timeout_secs=2,
    )
    try:
        yield ff.FinFeedHttpClient(config), requests, entered, release
    finally:
        release.set()
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def _response(name) -> bytes:
    return (TEST_DATA / name).read_bytes()


def _assert_fields(actual, expected, field=None) -> None:
    if field in ENUMS:
        assert actual == getattr(
            getattr(ff, ENUMS[field]),
            {"MultipleChoice": "MULTIPLE_CHOICE"}.get(expected, expected.upper()),
        )
    elif isinstance(expected, list):
        assert len(actual) == len(expected)
        for item, value in zip(actual, expected, strict=True):
            _assert_fields(item, value)
    elif isinstance(expected, dict) and not isinstance(actual, dict):
        for name in FIELDS[type(actual).__name__].split():
            _assert_fields(getattr(actual, name), expected.get(name), name)
    elif (
        isinstance(expected, str)
        and field is not None
        and (field.startswith("time_") or field in {"timestamp", "entry_time", "recv_time"})
    ):
        assert Timestamp(actual).value == Timestamp(expected).value
    else:
        assert actual == expected


@pytest.mark.parametrize(
    ("method", "args", "path", "fixture", "envelope"),
    [
        ("get_exchanges", (), "exchanges", "exchanges.json", None),
        ("get_exchange", ("MYRIAD",), "exchanges/MYRIAD", "exchange.json", None),
        (
            "get_listings",
            ("POLYMARKET", "listings"),
            "markets/POLYMARKET/history?limit=2",
            "polymarket-listings.json",
            None,
        ),
        (
            "get_active",
            ("POLYMARKET", "active"),
            "markets/POLYMARKET/active?limit=2&page=1",
            "active-one.json",
            None,
        ),
        (
            "get_market",
            ("MYRIAD", "Opaque/Outcome?+=%"),
            "markets/MYRIAD/Opaque%2FOutcome%3F+=%25",
            "exact-market.json",
            None,
        ),
        (
            "get_current",
            ("MYRIAD", "m"),
            "activity/MYRIAD/m/current",
            "exact-trade.json",
            "current",
        ),
        (
            "get_latest",
            ("MYRIAD", "m", "activity"),
            "activity/MYRIAD/m/latest?limit=2",
            "exact-quote.json",
            "latest",
        ),
        (
            "get_trades_history",
            ("MYRIAD", "m", "history"),
            "activity/MYRIAD/m/history/trades?date=2026-10-11&limit=2",
            "exact-trade.json",
            "array",
        ),
        (
            "get_quotes_history",
            ("MYRIAD", "m", "history"),
            "activity/MYRIAD/m/history/quotes?date=2026-10-11&limit=2",
            "exact-quote.json",
            "array",
        ),
        (
            "get_book",
            ("POLYMARKET", "m"),
            "orderbook/POLYMARKET/m/current",
            "polymarket-book.json",
            None,
        ),
        (
            "get_book_history",
            ("POLYMARKET", "m", "history"),
            "orderbook/POLYMARKET/m/history?date=2026-10-11&limit=2",
            "book-history-exact.json",
            None,
        ),
        ("get_periods", (), "ohlcv/periods", "periods.json", None),
        (
            "get_ohlcv_exchange_history",
            ("MYRIAD", "ohlcv_exchange"),
            "ohlcv/MYRIAD/history?period_id=1HRS&time_start=2026-10-11T00%3A00%3A00.0000001Z&time_end=2026-10-12T00%3A00%3A00.0000001Z&limit=2",
            "exchange-ohlcv-exact.json",
            None,
        ),
        (
            "get_ohlcv_history",
            ("POLYMARKET", "m", "ohlcv"),
            "ohlcv/POLYMARKET/m/history?period_id=1MIN&limit=2",
            "exact-bar.json",
            "array",
        ),
        (
            "get_ohlcv_latest",
            ("POLYMARKET", "m", "ohlcv"),
            "ohlcv/POLYMARKET/m/latest?period_id=1MIN&limit=2",
            "exact-bar.json",
            "array",
        ),
    ],
)
def test_read_only_operations_preserve_every_field(method, args, path, fixture, envelope) -> None:
    """
    Match every provider field, exact decimal, timestamp, route, and header.
    """
    queries = {
        "listings": ff.FinFeedListingsQuery(2),
        "active": ff.FinFeedActiveQuery(2, 1),
        "activity": ff.FinFeedActivityQuery(2),
        "history": ff.FinFeedHistoryQuery(2, date="2026-10-11"),
        "ohlcv": ff.FinFeedOhlcvQuery("1MIN", 2),
        "ohlcv_exchange": ff.FinFeedOhlcvQuery(
            "1HRS",
            2,
            time_start="2026-10-11T00:00:00.0000001Z",
            time_end="2026-10-12T00:00:00.0000001Z",
        ),
    }
    args = tuple(queries.get(arg, arg) for arg in args)
    body = _response(fixture)
    if envelope == "array":
        body = b"[" + body + b"]"
    elif envelope == "current":
        body = b'{"trade":' + body + b',"quote":' + _response("exact-quote.json") + b"}"
    elif envelope == "latest":
        body = b'{"quotes":[' + body + b'],"trades":[' + _response("exact-trade.json") + b"]}"
    expected = json.loads(body, parse_float=Decimal)
    with _peer([(200, body)]) as (client, requests, _, _):

        async def run():
            return await getattr(client, method)(*args)

        actual = asyncio.run(run())
        _assert_fields(actual, expected)
        assert requests == [(f"/v1/{path}", "python-rest-secret", "nautilus")]
        assert client.attempts_remaining == 29
        assert repr(client) == "FinFeedHttpClient"


def test_listing_traversal_converts_market_results() -> None:
    """
    Project collected Rust markets through the Python awaitable.
    """
    first = _response("polymarket-listings.json")
    with _peer([(200, first), (200, b'{"data":[],"next_cursor":null}')]) as (
        client,
        requests,
        _,
        _,
    ):

        async def run():
            return await client.get_listings_all(
                "POLYMARKET",
                ff.FinFeedListingsQuery(2),
                ff.FinFeedPagination(2, 2),
            )

        markets = asyncio.run(run())
        _assert_fields(markets, json.loads(first, parse_float=Decimal)["data"])
        assert [path for path, _, _ in requests] == [
            "/v1/markets/POLYMARKET/history?limit=2",
            "/v1/markets/POLYMARKET/history?limit=2&cursor=test-pair_YES",
        ]


def test_active_traversal_converts_string_results() -> None:
    """
    Project collected Rust IDs through the Python awaitable.
    """
    with _peer([(200, _response("active-one.json")), (200, b"[]")]) as (client, requests, _, _):

        async def run():
            return await client.get_active_all(
                "POLYMARKET",
                ff.FinFeedActiveQuery(2, 1),
                ff.FinFeedPagination(2, 1),
            )

        assert asyncio.run(run()) == ["opaque/ID?+=%"]
        assert [path for path, _, _ in requests] == [
            "/v1/markets/POLYMARKET/active?limit=2&page=1",
            "/v1/markets/POLYMARKET/active?limit=2&page=2",
        ]


@pytest.mark.parametrize(
    ("method", "query", "fixture", "message"),
    [
        (
            "get_listings_all",
            lambda: ff.FinFeedListingsQuery(2),
            "polymarket-listings.json",
            "listing page bound exceeded",
        ),
        (
            "get_active_all",
            lambda: ff.FinFeedActiveQuery(2, 1),
            "active-one.json",
            "active page bound exceeded",
        ),
    ],
)
def test_discovery_errors_convert_to_runtime_error(method, query, fixture, message) -> None:
    """
    Map errors from both Rust traversal methods to Python exceptions.
    """
    with _peer([(200, _response(fixture))]) as (client, requests, _, _):

        async def run():
            return await getattr(client, method)(
                "POLYMARKET",
                query(),
                ff.FinFeedPagination(1, 10),
            )

        with pytest.raises(RuntimeError, match=message):
            asyncio.run(run())
        assert len(requests) == 1


@pytest.mark.parametrize(
    ("query", "method", "message"),
    [
        (lambda: ff.FinFeedListingsQuery(0), "get_listings", "limit must be"),
        (lambda: ff.FinFeedActiveQuery(1, 0), "get_active", "page must be"),
    ],
)
def test_invalid_queries_fail_before_request(query, method, message) -> None:
    """
    Reject invalid query bounds before consuming an attempt.
    """
    with _peer([]) as (client, requests, _, _):

        async def run():
            return await getattr(client, method)("POLYMARKET", query())

        with pytest.raises(ValueError, match=message):
            asyncio.run(run())
        assert requests == []
        assert client.attempts_remaining == 30


def test_http_error_converts_to_redacted_runtime_error() -> None:
    """
    Expose the HTTP status without the credential echoed by the peer.
    """
    with _peer([(401, b"python-rest-secret")]) as (client, requests, _, _):

        async def run():
            return await client.get_exchanges()

        with pytest.raises(RuntimeError, match="FinFeed HTTP status 401") as caught:
            asyncio.run(run())
        assert "python-rest-secret" not in str(caught.value)
        assert requests == [("/v1/exchanges", "python-rest-secret", "nautilus")]


def test_malformed_response_is_redacted_and_budget_is_shared() -> None:
    """
    Keep parsing failures redacted and share the attempt budget across methods.
    """
    with _peer([(200, b"python-rest-secret")], attempt_budget=1) as (client, requests, _, _):

        async def run():
            with pytest.raises(RuntimeError, match="response parsing failed") as caught:
                await client.get_exchanges()
            assert "python-rest-secret" not in str(caught.value)
            with pytest.raises(RuntimeError, match="attempt budget exhausted"):
                await client.get_periods()

        asyncio.run(run())
        assert client.attempts_remaining == 0
        assert requests == [("/v1/exchanges", "python-rest-secret", "nautilus")]


@pytest.mark.parametrize("exchange_ohlcv", [False, True])
def test_cancellation_stops_pending_and_future_requests(exchange_ohlcv) -> None:
    """
    Permanently cancel in-flight, queued, and future calls.
    """
    with _peer([(200, _response("exchanges.json"))], block=True) as (
        client,
        requests,
        entered,
        release,
    ):

        async def run():
            operation = (
                client.get_ohlcv_exchange_history("POLYMARKET", _exchange_query())
                if exchange_ohlcv
                else client.get_exchanges()
            )
            pending = asyncio.ensure_future(operation)
            assert await asyncio.to_thread(entered.wait, 3)
            waiting = asyncio.ensure_future(client.get_periods())
            client.cancel()

            for future in (pending, waiting):
                with pytest.raises(RuntimeError, match="canceled"):
                    await asyncio.wait_for(future, 2)
            with pytest.raises(RuntimeError, match="canceled"):
                await client.get_exchanges()

        asyncio.run(run())
        release.set()
        expected_path = (
            "/v1/ohlcv/POLYMARKET/history?period_id=1HRS&time_start=2026-10-11T00%3A00%3A00.0000000Z&time_end=2026-10-12T00%3A00%3A00.0000000Z&limit=2"
            if exchange_ohlcv
            else "/v1/exchanges"
        )
        assert requests == [(expected_path, "python-rest-secret", "nautilus")]


def _exchange_query(**overrides: Any):
    values = {
        "period_id": "1HRS",
        "limit": 2,
        "time_start": "2026-10-11T00:00:00Z",
        "time_end": "2026-10-12T00:00:00Z",
    }
    values.update(overrides)
    return ff.FinFeedOhlcvQuery(**values)


@pytest.mark.parametrize(
    "overrides",
    [
        {"time_start": None},
        {"time_end": None},
        {"time_end": "2026-10-12T00:00:00.0000001Z"},
        {"limit": 0},
        {"limit": 10001},
        {"period_id": " "},
        {"time_end": "2026-10-11T00:00:00Z"},
    ],
)
def test_exchange_ohlcv_query_fails_before_admission(overrides) -> None:
    """
    Reject invalid exchange windows and caps before consuming an attempt.
    """
    with _peer([]) as (client, requests, _, _):

        async def run():
            return await client.get_ohlcv_exchange_history(
                "POLYMARKET",
                _exchange_query(**overrides),
            )

        with pytest.raises(ValueError, match=r"requires|24 hours|limit must|period_id|precede"):
            asyncio.run(run())
        assert requests == []
        assert client.attempts_remaining == 30


def test_canceling_one_python_future_releases_admission() -> None:
    """
    Release admission when Python cancels a single awaitable.
    """
    with _peer(
        [(200, _response("exchanges.json")), (200, _response("periods.json"))],
        block=True,
    ) as (
        client,
        requests,
        entered,
        release,
    ):

        async def run():
            pending = asyncio.ensure_future(client.get_exchanges())
            assert await asyncio.to_thread(entered.wait, 3)
            pending.cancel()
            with pytest.raises(asyncio.CancelledError):
                await pending
            release.set()
            periods = await asyncio.wait_for(client.get_periods(), 3)
            _assert_fields(periods, json.loads(_response("periods.json"), parse_float=Decimal))

        asyncio.run(run())
        assert [path for path, _, _ in requests] == ["/v1/exchanges", "/v1/ohlcv/periods"]


def test_raw_markets_remain_distinct_from_supported_instruments() -> None:
    """
    Retain raw metadata while checking Nautilus mappings separately.
    """
    with _peer(
        [(200, _response("exact-market.json")), (200, _response("polymarket-market.json"))],
    ) as (client, _, _, _):

        async def run():
            raw = await client.get_market("MYRIAD", "Opaque/Outcome?+=%")
            supported = await client.get_market("POLYMARKET", "m")
            return raw, supported

        raw, supported = asyncio.run(run())
        with pytest.raises(ValueError, match="outcome_type is unsupported"):
            raw.to_instrument()
        instrument = supported.to_instrument(ts_init=123456789)
        assert str(instrument.id) == f"{supported.market_id}.FINFEED_POLYMARKET"
        assert instrument.ts_init == 123456789
        assert instrument.price_precision == 4
        assert instrument.size_precision == 6
        assert raw.source_specific_data == {
            "precise": Decimal("0.9876543210987654321098765432"),
            "nested": {"token": "opaque/Token?+=%"},
        }


def test_time_queries_preserve_precision_and_reject_invalid_values() -> None:
    """
    Preserve 100 ns selectors and reject malformed time input.
    """
    start = "2026-10-11T00:00:00.0000001Z"
    end = "2026-10-11T00:00:01.0000002Z"
    history = ff.FinFeedHistoryQuery(3, time_start=start, time_end=end)
    ohlcv = ff.FinFeedOhlcvQuery("1MIN", 5, time_start=start, time_end=end)
    assert history.limit == 3
    assert history.date is None
    assert history.time_start == start
    assert history.time_end == end
    assert ohlcv.period_id == "1MIN"
    assert ohlcv.limit == 5
    assert ohlcv.time_start == start
    assert ohlcv.time_end == end
    with pytest.raises(ValueError, match="invalid time_start"):
        ff.FinFeedHistoryQuery(1, time_start="python-rest-secret")
    with pytest.raises(ValueError, match="invalid date"):
        ff.FinFeedHistoryQuery(1, date="python-rest-secret")


def test_generated_stub_matches_runtime_exports_and_result_properties() -> None:
    """
    Keep exports, typed awaitables, and every result property in generated stubs.
    """
    module_path = Path(ff.__file__)
    tree = ast.parse(module_path.with_suffix(".pyi").read_text(encoding="utf-8"))
    classes = {node.name: node for node in tree.body if isinstance(node, ast.ClassDef)}
    exports = next(
        ast.literal_eval(node.value)
        for node in tree.body
        if isinstance(node, ast.Assign)
        and any(isinstance(target, ast.Name) and target.id == "__all__" for target in node.targets)
    )
    assert exports == ff.__all__

    for name, fields in FIELDS.items():
        properties = {
            node.name
            for node in classes[name].body
            if isinstance(node, ast.FunctionDef)
            and any(
                isinstance(decorator, ast.Name) and decorator.id == "property"
                for decorator in node.decorator_list
            )
        }
        assert properties == set(fields.split())
        for field in fields.split():
            assert isinstance(vars(getattr(ff, name))[field], property) or hasattr(
                vars(getattr(ff, name))[field],
                "__get__",
            )
    methods = {
        node.name: ast.unparse(node.returns)
        for node in classes["FinFeedHttpClient"].body
        if isinstance(node, ast.FunctionDef) and node.name.startswith("get_")
    }
    assert set(methods) == {name for name in vars(ff.FinFeedHttpClient) if name.startswith("get_")}
    assert len(methods) == 18
    assert all(result.startswith("typing.Awaitable[") for result in methods.values())


def test_discovery_example_filters_raw_definitions_before_selection() -> None:
    """
    Select a checked definition after skipping an unsupported record.
    """
    example = runpy.run_path(str(TEST_DATA.parents[3] / "examples/live/finfeed/discovery.py"))
    supported = json.loads(_response("polymarket-market.json"))
    ids = json.dumps(["unsupported", supported["market_id"]]).encode()
    with _peer(
        [
            (200, ids),
            (200, _response("exact-market.json").replace(b'"Suspended"', b'"Open"')),
            (200, _response("polymarket-market.json")),
        ],
    ) as (client, requests, _, _):
        instrument = asyncio.run(example["discover"](client))
        selection = ff.FinFeedInstrumentSelection("POLYMARKET", [str(instrument.raw_symbol)])
        assert selection.exchange_id == "POLYMARKET"
        assert selection.market_ids == [supported["market_id"]]
        assert str(instrument.id) == f"{supported['market_id']}.FINFEED_POLYMARKET"
        assert [path for path, _, _ in requests] == [
            "/v1/markets/POLYMARKET/active?limit=3&page=1",
            "/v1/markets/POLYMARKET/unsupported",
            f"/v1/markets/POLYMARKET/{supported['market_id']}",
        ]


@pytest.mark.parametrize(
    ("field", "label", "enum_name"),
    [
        ("status", "Open", "FinFeedMarketStatus"),
        ("status", "Closed", "FinFeedMarketStatus"),
        ("status", "Resolved", "FinFeedMarketStatus"),
        ("status", "Suspended", "FinFeedMarketStatus"),
        ("outcome_type", "Binary", "FinFeedOutcomeType"),
        ("outcome_type", "MultipleChoice", "FinFeedOutcomeType"),
        ("outcome_type", "Numeric", "FinFeedOutcomeType"),
        ("side", "Buy", "FinFeedTradeSide"),
        ("side", "Sell", "FinFeedTradeSide"),
        ("status", "FutureStatus", "FinFeedMarketStatus"),
        ("outcome_type", "FutureOutcome", "FinFeedOutcomeType"),
        ("side", "FutureSide", "FinFeedTradeSide"),
    ],
)
def test_enum_projections_preserve_every_variant(field, label, enum_name) -> None:
    """
    Preserve all provider enum labels without inferring native semantics.
    """
    if field == "side":
        body = _response("exact-trade.json").replace(b'"side":"Buy"', f'"side":"{label}"'.encode())
        body = b'{"trade":' + body + b"}"
    else:
        original = "Suspended" if field == "status" else "MultipleChoice"
        body = _response("exact-market.json").replace(
            f'"{field}":"{original}"'.encode(),
            f'"{field}":"{label}"'.encode(),
        )
    with _peer([(200, body)]) as (client, _, _, _):

        async def run():
            if field == "side":
                return (await client.get_current("MYRIAD", "m")).trade
            return await client.get_market("MYRIAD", "m")

        actual = asyncio.run(run())
        variant = {
            "MultipleChoice": "MULTIPLE_CHOICE",
            "FutureStatus": "UNKNOWN",
            "FutureOutcome": "OTHER",
            "FutureSide": "OTHER",
        }.get(label, label.upper())
        assert getattr(actual, field) == getattr(getattr(ff, enum_name), variant)


def test_unknown_metadata_keeps_siblings_and_checked_mapping() -> None:
    """
    Retain unknown classifications while rejecting unsupported instrument semantics.
    """
    known = _response("polymarket-market.json")
    status = known.replace(b'"Open"', b'"FutureStatus"')
    outcome = known.replace(b'"Binary"', b'"FutureOutcome"')
    body = b'{"data":[' + b",".join([known, status, outcome]) + b'],"next_cursor":"next"}'
    expected = json.loads(body, parse_float=Decimal)
    expected["data"][1]["status"] = "Unknown"
    expected["data"][2]["outcome_type"] = "Other"
    with _peer([(200, body)]) as (client, _, _, _):

        async def run():
            return await client.get_listings("POLYMARKET", ff.FinFeedListingsQuery(3))

        page = asyncio.run(run())
        _assert_fields(page, expected)
        instrument = page.data[1].to_instrument(ts_init=123)
        assert str(instrument.id) == f"{page.data[0].market_id}.FINFEED_POLYMARKET"
        assert instrument.ts_init == 123
        assert instrument.expiration_ns == page.data[0].to_instrument().expiration_ns
        with pytest.raises(ValueError, match="outcome_type is unsupported") as exc:
            page.data[2].to_instrument()
        assert str(exc.value) == (
            f"Invalid FinFeed definition POLYMARKET/{page.data[2].market_id}: "
            "outcome_type is unsupported"
        )


@pytest.mark.parametrize("current", [False, True])
def test_unknown_trade_side_keeps_every_activity_field(current) -> None:
    """
    Retain exact trades and sibling quotes when a side label is unknown.
    """
    known = _response("exact-trade.json")
    unknown = known.replace(b'"Buy"', b'"FutureSide"')
    quote = _response("exact-quote.json")
    if current:
        body = b'{"trade":' + unknown + b',"quote":' + quote + b"}"
    else:
        body = b'{"trades":[' + unknown + b"," + known + b'],"quotes":[' + quote + b"]}"
    expected = json.loads(body, parse_float=Decimal)
    if current:
        expected["trade"]["side"] = "Other"
    else:
        expected["trades"][0]["side"] = "Other"
    with _peer([(200, body)]) as (client, _, _, _):

        async def run():
            if current:
                return await client.get_current("MYRIAD", "m")
            return await client.get_latest("MYRIAD", "m", ff.FinFeedActivityQuery(2))

        _assert_fields(asyncio.run(run()), expected)


@pytest.mark.parametrize(
    "kind",
    [
        "exchange",
        "market",
        "trade",
        "book",
        "period",
        "empty_current",
        "empty_batch",
        "empty_listings",
        "bar",
    ],
)
def test_nullable_results_preserve_missing_values(kind) -> None:
    """
    Keep omitted and null fields distinct from empty lists or invented values.
    """
    nullable = json.loads(_response("nullable-identifiers.json"))
    if kind in nullable:
        value = nullable[kind]
        if kind == "trade":
            value = {"trade": value}
        elif kind == "period":
            value = [value]
        body = json.dumps(value).encode()
    elif kind == "bar":
        body = _response("exact-bar.json")

        for field, value in [
            ("price_open", "0.1"),
            ("price_high", "0.9"),
            ("price_low", "0.01"),
            ("price_close", "0.5"),
        ]:
            body = body.replace(f'"{field}":{value}'.encode(), f'"{field}":null'.encode())
        for field, value in [
            ("time_open", "2026-10-11T00:00:02.0000003Z"),
            ("time_close", "2026-10-11T00:00:58.0000004Z"),
        ]:
            body = body.replace(f'"{field}":"{value}"'.encode(), f'"{field}":null'.encode())
        body = b"[" + body + b"]"
    else:
        value = {
            "empty_current": {},
            "empty_batch": {"trades": None, "quotes": []},
            "empty_listings": {"data": None, "next_cursor": None},
        }[kind]
        body = json.dumps(value).encode()
    with _peer([(200, body)]) as (client, _, _, _):
        operations = {
            "exchange": lambda: client.get_exchange("MYRIAD"),
            "market": lambda: client.get_market("MYRIAD", "m"),
            "trade": lambda: client.get_current("MYRIAD", "m"),
            "book": lambda: client.get_book("MYRIAD", "m"),
            "period": client.get_periods,
            "empty_current": lambda: client.get_current("MYRIAD", "m"),
            "empty_batch": lambda: client.get_latest("MYRIAD", "m", ff.FinFeedActivityQuery(1)),
            "empty_listings": lambda: client.get_listings("MYRIAD", ff.FinFeedListingsQuery(1)),
            "bar": lambda: client.get_ohlcv_latest("MYRIAD", "m", ff.FinFeedOhlcvQuery("1MIN", 1)),
        }

        async def run():
            return await operations[kind]()

        _assert_fields(asyncio.run(run()), json.loads(body, parse_float=Decimal))


@pytest.mark.parametrize(
    ("factory", "expected"),
    [
        (
            lambda: ff.FinFeedListingsQuery(17, "opaque/Next?+=%"),
            {"limit": 17, "cursor": "opaque/Next?+=%"},
        ),
        (lambda: ff.FinFeedActiveQuery(19, 23), {"limit": 19, "page": 23}),
        (lambda: ff.FinFeedActivityQuery(29), {"limit": 29}),
        (lambda: ff.FinFeedPagination(31, 37), {"max_pages": 31, "max_records": 37}),
        (
            lambda: ff.FinFeedHistoryQuery(41, date="2026-10-11"),
            {"limit": 41, "date": "2026-10-11", "time_start": None, "time_end": None},
        ),
    ],
)
def test_query_properties_preserve_explicit_inputs(factory, expected) -> None:
    """
    Return distinct configured query fields without swaps or defaults.
    """
    query = factory()
    assert {name: getattr(query, name) for name in expected} == expected


@pytest.mark.parametrize(
    ("factory", "message"),
    [
        (lambda: ff.FinFeedHistoryQuery(1, time_end="python-rest-secret"), "invalid time_end"),
        (
            lambda: ff.FinFeedOhlcvQuery("1MIN", 1, time_end="python-rest-secret"),
            "invalid time_end",
        ),
        (
            lambda: ff.FinFeedHttpClient(
                ff.FinFeedDataClientConfig(api_key="python-rest-secret", attempt_budget=0),
            ),
            "request controls must be positive",
        ),
        (
            lambda: ff.FinFeedHttpClient(
                ff.FinFeedDataClientConfig(
                    api_key="python-rest-secret",
                    base_url_http="http://user:python-rest-secret@localhost:8123",
                ),
            ),
            "base URL must be HTTPS",
        ),
        (
            lambda: ff.FinFeedHttpClient(
                ff.FinFeedDataClientConfig(
                    api_key="python-rest-secret",
                    proxy_url="http://user:python-rest-secret@localhost:8124/path",
                ),
            ),
            "proxy must be HTTP",
        ),
    ],
)
def test_invalid_python_controls_redact_credentials(factory, message) -> None:
    """
    Use ValueError with sanitized messages for invalid selectors and transport controls.
    """
    with pytest.raises(ValueError, match=message) as caught:
        factory()
    assert "python-rest-secret" not in str(caught.value)


@pytest.mark.parametrize("kind", ["empty", "closed", "unsupported"])
def test_discovery_example_rejects_unsupported_sample(kind) -> None:
    """
    Stop before node construction if the sample has no supported open definition.
    """
    example = runpy.run_path(str(TEST_DATA.parents[3] / "examples/live/finfeed/discovery.py"))
    responses = [(200, b"[]" if kind == "empty" else json.dumps([kind]).encode())]
    if kind == "closed":
        responses.append((200, _response("polymarket-market.json").replace(b'"Open"', b'"Closed"')))
    elif kind == "unsupported":
        responses.append((200, _response("exact-market.json").replace(b'"Suspended"', b'"Open"')))
    with _peer(responses) as (client, requests, _, _):
        with pytest.raises(RuntimeError, match="No supported open outcome in the discovery sample"):
            asyncio.run(example["discover"](client))
        assert len(requests) == len(responses)


@pytest.mark.parametrize("period", ["1SEC", "1MIN", "1HRS"])
@pytest.mark.parametrize("selector_kind", ["date", "range", "start"])
def test_flat_file_ohlcv_preserves_source_selectors_and_exact_fields(period, selector_kind) -> None:
    """
    Preserve every raw field and keep flat-file selectors separate from DB OHLCV.
    """
    start = "2026-10-11T00:00:00.0000001Z"
    end = "2026-10-11T01:00:00.0000002Z"
    selectors = {"date": "2026-10-11"} if selector_kind == "date" else {"time_start": start}
    if selector_kind == "range":
        selectors["time_end"] = end
    query = ff.FinFeedFlatFileOhlcvQuery(period, 10000, **selectors)
    body = b"[" + _response("exact-bar.json") + b"]"
    with _peer([(200, body), (200, b"[]")]) as (client, requests, _, _):

        async def run():
            flat = await client.get_ohlcv_flat_file_history("KAL/SHI", "Mixed?NO", query)
            db = await client.get_ohlcv_history(
                "KAL/SHI",
                "Mixed?NO",
                ff.FinFeedOhlcvQuery("1MIN", 1),
            )
            return flat, db

        flat, db = asyncio.run(run())
        _assert_fields(flat, json.loads(body, parse_float=Decimal))
        assert db == []
        selector = (
            "date=2026-10-11"
            if selector_kind == "date"
            else "time_start=2026-10-11T00%3A00%3A00.0000001Z"
        )

        if selector_kind == "range":
            selector += "&time_end=2026-10-11T01%3A00%3A00.0000002Z"
        assert requests == [
            (
                f"/v1/activity/KAL%2FSHI/Mixed%3FNO/history/ohlcv?period={period}&{selector}&limit=10000",
                "python-rest-secret",
                "nautilus",
            ),
            (
                "/v1/ohlcv/KAL%2FSHI/Mixed%3FNO/history?period_id=1MIN&limit=1",
                "python-rest-secret",
                "nautilus",
            ),
        ]
        assert client.attempts_remaining == 28
        assert query.period == period
        assert query.limit == 10000
        assert query.date == ("2026-10-11" if selector_kind == "date" else None)
        assert query.time_start == (None if selector_kind == "date" else start)
        assert query.time_end == (end if selector_kind == "range" else None)


@pytest.mark.parametrize(
    ("overrides", "message"),
    [
        ({"period": "1DAY"}, "flat-file period"),
        ({"period": ""}, "flat-file period"),
        ({"limit": 0}, "limit must"),
        ({"limit": 10001}, "limit must"),
        ({"date": None}, "date or time_start"),
        ({"time_start": "2026-10-11T00:00:00Z"}, "mutually exclusive"),
        ({"date": None, "time_end": "2026-10-11T01:00:00Z"}, "date or time_start"),
        (
            {
                "date": None,
                "time_start": "2026-10-11T01:00:00Z",
                "time_end": "2026-10-11T00:00:00Z",
            },
            "precede",
        ),
        ({"date": None, "time_start": "2026-10-11T00:00:00.000000001Z"}, "100 ns precision"),
    ],
)
def test_flat_file_ohlcv_validation_precedes_admission(overrides, message) -> None:
    """
    Reject invalid flat-file selectors without consuming an attempt.
    """
    values = {"period": "1MIN", "limit": 3, "date": "2026-10-11"} | overrides
    with _peer([]) as (client, requests, _, _):

        async def run():
            return await client.get_ohlcv_flat_file_history(
                "KALSHI",
                "m",
                ff.FinFeedFlatFileOhlcvQuery(**values),
            )

        with pytest.raises(ValueError, match=message):
            asyncio.run(run())
        assert requests == []
        assert client.attempts_remaining == 30


@pytest.mark.parametrize(
    ("status", "body", "message"),
    [
        (404, b"[]", "HTTP status 404"),
        (400, b"[]", "HTTP status 400"),
        (200, b"invalid", "parsing failed"),
        (200, b"overflow", "response exceeds requested record limit"),
        (200, b"[]", None),
    ],
)
def test_flat_file_ohlcv_failures_and_empty_never_fall_back(status, body, message) -> None:
    """
    Return errors and empty responses without querying another source.
    """
    if body == b"overflow":
        body = b"[" + _response("exact-bar.json") + b"," + _response("exact-bar.json") + b"]"
    with _peer([(status, body)]) as (client, requests, _, _):

        async def run():
            return await client.get_ohlcv_flat_file_history(
                "KALSHI",
                "m",
                ff.FinFeedFlatFileOhlcvQuery("1MIN", 1, date="2026-10-11"),
            )

        if message is None:
            assert asyncio.run(run()) == []
        else:
            with pytest.raises(RuntimeError, match=message):
                asyncio.run(run())
        assert requests == [
            (
                "/v1/activity/KALSHI/m/history/ohlcv?period=1MIN&date=2026-10-11&limit=1",
                "python-rest-secret",
                "nautilus",
            ),
        ]
        assert client.attempts_remaining == 29


@pytest.mark.parametrize(
    ("selectors", "message"),
    [
        ({"date": "bad"}, "invalid date"),
        ({"time_start": "bad"}, "invalid time_start"),
        ({"time_end": "bad"}, "invalid time_end"),
    ],
)
def test_flat_file_ohlcv_constructor_rejects_malformed_selectors(selectors, message) -> None:
    """
    Reject malformed dates and times at construction.
    """
    with pytest.raises(ValueError, match=message):
        ff.FinFeedFlatFileOhlcvQuery("1MIN", 1, **selectors)


@pytest.mark.parametrize("source", ["market", "flat-file", "exchange"])
@pytest.mark.parametrize("result", ["rows", "empty", "saturated", "http-error"])
def test_historical_research_example_keeps_selected_source(
    source,
    result,
    monkeypatch,
    capsys,
) -> None:
    """
    Preserve source routing, exact raw values, cancellation, and no fallback.
    """
    example = runpy.run_path(
        str(TEST_DATA.parents[3] / "examples/live/finfeed/historical_research.py"),
    )
    main = example["main"]
    status = 429 if result == "http-error" else 200
    body = b"[]" if result in ("empty", "http-error") else _response("exchange-ohlcv-exact.json")
    configs: list[ff.FinFeedDataClientConfig] = []
    with _peer([(status, body)], attempt_budget=1) as (client, requests, _, _):

        def create_client(config: ff.FinFeedDataClientConfig) -> ff.FinFeedHttpClient:
            configs.append(config)
            return client

        monkeypatch.setitem(main.__globals__, "FinFeedHttpClient", create_client)
        if result == "saturated":
            monkeypatch.setitem(main.__globals__, "LIMIT", 2)
        if result == "http-error":
            with pytest.raises(RuntimeError, match="429"):
                asyncio.run(main(source))
        else:
            asyncio.run(main(source))
        output = capsys.readouterr().out
        expected_controls = {
            "attempt_budget": 1,
            "http_timeout_secs": 20,
            "max_concurrent_requests": 1,
            "min_request_interval_ms": 10_000,
            "max_retries": 0,
        }
        assert len(configs) == 1
        assert {name: getattr(configs[0], name) for name in expected_controls} == expected_controls
        limit = 2 if result == "saturated" else 5
        market_id = example["MARKET_ID"]
        if source == "flat-file":
            endpoint = f"activity/POLYMARKET/{market_id}/history/ohlcv?period=1HRS"
        elif source == "exchange":
            endpoint = "ohlcv/POLYMARKET/history?period_id=1HRS"
        else:
            endpoint = f"ohlcv/POLYMARKET/{market_id}/history?period_id=1HRS"
        assert requests == [
            (
                f"/v1/{endpoint}&time_start=2026-10-11T00%3A00%3A00.0000000Z"
                f"&time_end=2026-10-12T00%3A00%3A00.0000000Z&limit={limit}",
                "python-rest-secret",
                "nautilus",
            ),
        ]
        assert client.attempts_remaining == 0

        if result == "http-error":
            assert output == ""
        elif result == "empty":
            assert output == (
                f"Source: {source}; rows: 0; remaining attempts: 0\n"
                "Empty sample; source coverage is unverified\n"
            )
        else:
            expected = (
                f"Source: {source}; rows: 2; remaining attempts: 0\n"
                "MixedCase/Outcome?+=%_NO 2026-10-11T00:00:00.0000001Z "
                "2026-10-11T00:01:00.0000002Z 0.5 9007199254740993.125\n"
                "unknown/YES 2026-10-12T00:00:00.0000001Z "
                "2026-10-12T01:00:00.0000002Z 0.5432109876543210987654321098 "
                "18446744073709551617.125\n"
            )

            if result == "saturated":
                expected += (
                    "Sample limit reached; more rows may exist, with no verified continuation\n"
                )
            assert output == expected

        async def check_canceled() -> None:
            with pytest.raises(RuntimeError, match="canceled"):
                await client.get_exchanges()

        asyncio.run(check_canceled())
