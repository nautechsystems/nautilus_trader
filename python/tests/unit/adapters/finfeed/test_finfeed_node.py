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
FinFeed factory lifecycle through the Python live node and a local REST peer.
"""

import asyncio
import json
import runpy
import threading
from collections.abc import Callable
from collections.abc import Iterator
from contextlib import suppress
from datetime import UTC
from datetime import datetime
from decimal import Decimal
from http.server import BaseHTTPRequestHandler
from http.server import ThreadingHTTPServer
from pathlib import Path
from queue import Empty
from queue import Queue
from time import monotonic_ns

import pytest

from nautilus_trader.adapters.finfeed import FINFEED_CLIENT_ID
from nautilus_trader.adapters.finfeed import FinFeedDataClientConfig
from nautilus_trader.adapters.finfeed import FinFeedDataClientFactory
from nautilus_trader.adapters.finfeed import FinFeedHttpClient
from nautilus_trader.adapters.finfeed import FinFeedInstrumentSelection
from nautilus_trader.common import DataActor
from nautilus_trader.common import Environment
from nautilus_trader.config import DataActorConfig
from nautilus_trader.config import ImportableControllerConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import NodeState
from nautilus_trader.model import ActorId
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import BarType
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderBook
from nautilus_trader.model import RecordFlag
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig
from nautilus_trader.trading import Controller


MARKET_ID = "TEST-OUTCOME_NO"
INSTRUMENT_ID = InstrumentId.from_str(f"{MARKET_ID}.FINFEED_POLYMARKET")
TEST_DATA = Path(__file__).resolve().parents[5] / "crates/adapters/finfeed/test_data"
BAR_TYPE = BarType.from_str(f"{INSTRUMENT_ID}-1-TICK-LAST-INTERNAL")


class Consumer(DataActor):
    """
    Observe instrument responses and sampled quotes.
    """

    def __init__(self) -> None:
        """
        Retain the observations for assertions after shutdown.
        """
        super().__init__()
        self.instruments = []
        self.quotes = []

    def on_start(self) -> None:
        """
        Request a fresh definition.
        """
        self.request_instrument(INSTRUMENT_ID, client_id=FINFEED_CLIENT_ID)

    def on_instrument(self, instrument) -> None:
        """
        Record the fresh definition and start quote polling.
        """
        self.instruments.append(instrument)
        self.subscribe_quotes(INSTRUMENT_ID, client_id=FINFEED_CLIENT_ID)

    def on_quote(self, quote) -> None:
        """
        Record sampled quotes.
        """
        self.quotes.append(quote)


def test_discovery_replay_subscriptions_and_fresh_node_restart() -> None:  # noqa: C901 - One selection crosses both node lifecycles
    """
    Carry a discovered selection through replay, polling, shutdown, and a fresh node.
    """
    requests = []
    discovery = runpy.run_path(str(TEST_DATA.parents[3] / "examples/live/finfeed/discovery.py"))
    replay = runpy.run_path(str(TEST_DATA.parents[3] / "examples/live/finfeed/historical_book.py"))
    actor_type = replay["HistoricalBook"]
    state = actor_type.on_start.__globals__
    state["INSTRUMENT_ID"] = INSTRUMENT_ID
    state["START"] = datetime(2026, 10, 11, 0, 0, 1, 1, tzinfo=UTC)
    state["END"] = datetime(2026, 10, 11, 0, 0, 6, tzinfo=UTC)

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            requests.append(
                (self.path, self.headers.get("Authorization"), self.headers.get("X-Referrer")),
            )

            if self.path == "/v1/markets/POLYMARKET/active?limit=3&page=1":
                body = json.dumps([MARKET_ID]).encode()
            elif self.path == "/v1/exchanges":
                body = (TEST_DATA / "exchanges.json").read_bytes()
            elif self.path.startswith("/v1/markets/"):
                body = (TEST_DATA / "polymarket-market.json").read_bytes()
            elif self.path.startswith(f"/v1/orderbook/POLYMARKET/{MARKET_ID}/history?"):
                body = (TEST_DATA / "book-replay.json").read_bytes()
            elif "/latest" in self.path:
                current = json.loads((TEST_DATA / "polymarket-current.json").read_bytes())
                body = json.dumps({"quotes": [current["quote"]], "trades": []}).encode()
            else:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_args: object) -> None:
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    async def exercise() -> tuple:
        controls = {
            "api_key": "node-test-key",
            "base_url_http": f"http://127.0.0.1:{server.server_port}/v1/",
            "min_request_interval_ms": 1,
            "max_retries": 0,
        }
        client = FinFeedHttpClient(FinFeedDataClientConfig(attempt_budget=2, **controls))
        try:
            instrument = await discovery["discover"](client)
        finally:
            client.cancel()

        results = []

        for _ in range(2):
            config = FinFeedDataClientConfig(
                instruments=[
                    FinFeedInstrumentSelection("POLYMARKET", [str(instrument.raw_symbol)]),
                ],
                poll_interval_ms=60_000,
                poll_limit=3,
                attempt_budget=5,
                **controls,
            )
            node = (
                LiveNode.builder("FINFEED-NODE", TraderId("TESTER-001"), Environment.LIVE)
                .with_reconciliation(False)
                .with_timeout_connection(5)
                .with_timeout_portfolio(0)
                .with_delay_post_stop_secs(0)
                .with_delay_shutdown_secs(0)
                .add_data_client(None, FinFeedDataClientFactory(), config)
                .build()
            )
            consumer = Consumer()
            historical = actor_type()
            node.add_actor(consumer)
            node.add_actor(historical)
            handle = node.handle()
            task = asyncio.create_task(node.run_async())

            try:
                try:
                    async with asyncio.timeout(10):
                        await historical.received.wait()

                        while not consumer.instruments or not consumer.quotes:
                            await asyncio.sleep(0.01)
                finally:
                    handle.stop()
                    await asyncio.wait_for(task, timeout=10)

                quotes = node.cache.quotes(instrument.id)
                await asyncio.sleep(0.05)
                results.append(
                    (
                        handle.state,
                        consumer,
                        historical,
                        quotes,
                        node.cache.quotes(instrument.id),
                    ),
                )
            finally:
                node.dispose()
                del node

        return instrument, results

    try:
        instrument, results = asyncio.run(exercise())
        assert instrument.id == INSTRUMENT_ID
        assert len(results) == 2

        for stopped, consumer, historical, before, after in results:
            assert stopped == NodeState.STOPPED
            [loaded] = consumer.instruments
            expected = instrument.to_dict()
            actual = loaded.to_dict()
            actual["ts_event"] = expected["ts_event"]
            actual["ts_init"] = expected["ts_init"]
            assert actual == expected
            assert historical.batches == 5
            assert historical.book.bids_to_dict() == {Decimal(".2"): Decimal(6)}
            assert historical.book.asks_to_dict() == {Decimal(".9"): Decimal(10)}
            [quote] = consumer.quotes
            assert before == after == [quote]
            assert quote.instrument_id == INSTRUMENT_ID
            assert str(quote.bid_price) == "0.2000"
            assert str(quote.ask_price) == "0.3000"
            assert str(quote.bid_size) == "11.000000"
            assert str(quote.ask_size) == "21.000000"
            assert quote.ts_event == 1_791_763_260_000_000_100
            assert quote.ts_init == 1_791_763_260_000_000_300

        paths = [path for path, _, _ in requests]
        assert paths[:2] == [
            "/v1/markets/POLYMARKET/active?limit=3&page=1",
            f"/v1/markets/POLYMARKET/{MARKET_ID}",
        ]
        expected_paths = [
            "/v1/exchanges",
            f"/v1/markets/POLYMARKET/{MARKET_ID}",
            f"/v1/markets/POLYMARKET/{MARKET_ID}",
            f"/v1/orderbook/POLYMARKET/{MARKET_ID}/history?"
            "time_start=2026-10-11T00%3A00%3A00.0000000Z"
            "&time_end=2026-10-11T01%3A00%3A00.0000000Z&limit=10000",
            f"/v1/activity/POLYMARKET/{MARKET_ID}/latest?limit=3",
        ]
        assert len(paths) == 12

        for generation_paths in (paths[2:7], paths[7:]):
            assert generation_paths[:2] == expected_paths[:2]
            assert sorted(generation_paths[2:]) == sorted(expected_paths[2:])
            assert generation_paths.index(expected_paths[-1]) > generation_paths.index(
                expected_paths[2],
                2,
            )

        assert [(auth, referrer) for _, auth, referrer in requests] == [
            ("node-test-key", "nautilus"),
        ] * len(requests)
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def test_builtin_data_tester_shares_activity_and_stops(polling_node) -> None:
    """
    Run the built-in tester through exact shared quote/trade delivery and shutdown.
    """
    node, observed, requests = polling_node
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            actor_id=ActorId("Builtin-DataTester"),
            client_id=FINFEED_CLIENT_ID,
            instrument_ids=[INSTRUMENT_ID],
            subscribe_quotes=True,
            subscribe_trades=True,
            stats_interval_secs=0,
        ),
    )
    handle = node.handle()
    endpoint = f"/v1/activity/POLYMARKET/{MARKET_ID}/latest?limit=3"
    body = (TEST_DATA / "poll-activity.json").read_bytes()
    cache = node.cache

    async def exercise() -> bool:
        task = asyncio.create_task(node.run_async())
        try:
            path, reply, _ = await asyncio.to_thread(observed.get, True, 5)
            assert path == endpoint
            reply.put(body)
            await _wait_for(
                lambda: (
                    cache.quote_count(INSTRUMENT_ID) == 2 and cache.trade_count(INSTRUMENT_ID) == 2
                ),
            )
            path, reply, _ = await asyncio.to_thread(observed.get, True, 5)
            assert path == endpoint
        finally:
            handle.stop()
            await asyncio.wait_for(task, timeout=10)
        stopped_ns = monotonic_ns()
        reply.put(body)
        try:
            path, reply, received_ns = await asyncio.to_thread(observed.get, True, 0.35)
        except Empty:
            return False
        assert path == endpoint
        assert received_ns < stopped_ns
        reply.put(body)
        with pytest.raises(Empty):
            await asyncio.to_thread(observed.get, True, 0.35)
        return True

    poll_during_shutdown = asyncio.run(exercise())
    quotes = cache.quotes(INSTRUMENT_ID)
    trades = cache.trades(INSTRUMENT_ID)

    assert handle.state == NodeState.STOPPED
    assert [
        (
            quote.instrument_id,
            str(quote.bid_price),
            str(quote.ask_price),
            str(quote.bid_size),
            str(quote.ask_size),
            quote.ts_event,
            quote.ts_init,
        )
        for quote in quotes
    ] == [
        (
            INSTRUMENT_ID,
            "0.3100",
            "0.4100",
            "7.000007",
            "8.000008",
            1_791_676_801_000_000_000,
            1_791_676_802_000_000_000,
        ),
        (
            INSTRUMENT_ID,
            "0.3000",
            "0.4000",
            "3.000003",
            "4.000004",
            1_791_676_801_000_000_000,
            1_791_676_802_000_000_000,
        ),
    ]
    assert [
        (
            trade.instrument_id,
            str(trade.trade_id),
            str(trade.price),
            str(trade.size),
            trade.aggressor_side,
            trade.ts_event,
            trade.ts_init,
        )
        for trade in trades
    ] == [
        (
            INSTRUMENT_ID,
            "trade-3",
            "0.3200",
            "4.000004",
            AggressorSide.NO_AGGRESSOR,
            1_791_676_802_000_000_000,
            1_791_676_802_000_000_000,
        ),
        (
            INSTRUMENT_ID,
            "trade-1",
            "0.3100",
            "2.000002",
            AggressorSide.NO_AGGRESSOR,
            1_791_676_802_000_000_000,
            1_791_676_802_000_000_000,
        ),
    ]
    assert [path for path, _, _ in requests] == [
        "/v1/exchanges",
        f"/v1/markets/POLYMARKET/{MARKET_ID}",
        endpoint,
        endpoint,
    ] + [endpoint] * poll_during_shutdown


@pytest.mark.parametrize("feed", ["quotes", "book"])
def test_polling_actor_stop_releases_rest_demand(polling_node, feed: str) -> None:
    """
    Publish exact quotes or interval books, then release REST demand on actor stop.
    """
    node, observed, requests = polling_node
    consumer = PollingConsumer(DataActorConfig(actor_id=ActorId("Stop-Consumer")))
    consumer.feed = feed
    node.add_actor(consumer)
    handle = node.handle()
    endpoint = (
        f"/v1/orderbook/POLYMARKET/{MARKET_ID}/current"
        if feed == "book"
        else f"/v1/activity/POLYMARKET/{MARKET_ID}/latest?limit=3"
    )
    fixture = "poll-book.json" if feed == "book" else "poll-activity.json"
    body = (TEST_DATA / fixture).read_bytes()

    async def exercise() -> None:
        task = asyncio.create_task(node.run_async())
        try:
            path, reply, _ = await asyncio.to_thread(observed.get, True, 5)
            assert path == endpoint
            reply.put(body)
            await _wait_for(lambda: bool(consumer.events))
            path, reply, _ = await asyncio.to_thread(observed.get, True, 5)
            assert path == endpoint
            PollingController.current.stop_actor(consumer.actor_id)
            await _wait_for(consumer.is_stopped)
            reply.put(body)
            with pytest.raises(Empty):
                await asyncio.to_thread(observed.get, True, 0.35)
        finally:
            handle.stop()
            await asyncio.wait_for(task, timeout=10)

    asyncio.run(exercise())
    assert handle.state == NodeState.STOPPED

    if feed == "book":
        assert consumer.events == [
            (
                INSTRUMENT_ID,
                BookType.L2_MBP,
                0,
                6,
                1_791_676_801_000_000_000,
                [
                    (Decimal("0.3"), Decimal("1.000001")),
                    (Decimal("0.2"), Decimal("2.000002")),
                    (Decimal("0.1"), Decimal("3.000003")),
                ],
                [
                    (Decimal("0.4"), Decimal("4.000004")),
                    (Decimal("0.5"), Decimal("5.000005")),
                ],
            ),
        ] * len(consumer.events)
    else:
        assert [
            (
                quote.instrument_id,
                str(quote.bid_price),
                str(quote.ask_price),
                str(quote.bid_size),
                str(quote.ask_size),
                quote.ts_event,
                quote.ts_init,
            )
            for quote in consumer.events
        ] == [
            (
                INSTRUMENT_ID,
                "0.3000",
                "0.4000",
                "3.000003",
                "4.000004",
                1_791_676_801_000_000_000,
                1_791_676_802_000_000_000,
            ),
            (
                INSTRUMENT_ID,
                "0.3100",
                "0.4100",
                "7.000007",
                "8.000008",
                1_791_676_801_000_000_000,
                1_791_676_802_000_000_000,
            ),
        ]
    assert [path for path, _, _ in requests] == [
        "/v1/exchanges",
        f"/v1/markets/POLYMARKET/{MARKET_ID}",
        endpoint,
        endpoint,
    ]


def test_retirement_preserves_shared_internal_bars_until_final_subscriber(polling_node) -> None:
    """
    Retire shared bar consumers while FinFeed trade polling is in flight.
    """
    node, observed, requests = polling_node
    first = PollingConsumer(DataActorConfig(actor_id=ActorId("Retired-Consumer")))
    second = PollingConsumer(DataActorConfig(actor_id=ActorId("Remaining-Consumer")))
    first.feed = "bars"
    second.feed = "bars"
    node.add_actor(first)
    node.add_actor(second)
    handle = node.handle()
    activity = json.loads((TEST_DATA / "poll-activity.json").read_bytes())
    endpoint = f"/v1/activity/POLYMARKET/{MARKET_ID}/latest?limit=3"

    async def exercise() -> None:
        task = asyncio.create_task(node.run_async())
        try:
            path, reply, _ = await asyncio.to_thread(observed.get, True, 5)
            assert path == endpoint
            reply.put(json.dumps(activity).encode())
            await _wait_for(lambda: len(first.events) == 2 and len(second.events) == 2)
            path, reply, _ = await asyncio.to_thread(observed.get, True, 5)
            assert path == endpoint
            PollingController.current.remove_actor(first.actor_id)
            await _wait_for(first.is_disposed)

            for trade in activity["trades"]:
                trade["id"] += "-next"
                trade["timestamp"] = "2026-10-11T00:00:04Z"
            reply.put(json.dumps(activity).encode())
            await _wait_for(lambda: len(second.events) == 4)
            assert len(first.events) == 2
            path, reply, _ = await asyncio.to_thread(observed.get, True, 5)
            assert path == endpoint
            PollingController.current.remove_actor(second.actor_id)
            await _wait_for(second.is_disposed)

            for trade in activity["trades"]:
                trade["id"] += "-late"
                trade["timestamp"] = "2026-10-11T00:00:06Z"
            reply.put(json.dumps(activity).encode())
            with pytest.raises(Empty):
                await asyncio.to_thread(observed.get, True, 0.35)
        finally:
            handle.stop()
            await asyncio.wait_for(task, timeout=10)

    asyncio.run(exercise())
    expected = [
        ("0.3100", "2.000002", 1_791_676_802_000_000_000),
        ("0.3200", "4.000004", 1_791_676_802_000_000_000),
        ("0.3100", "2.000002", 1_791_676_804_000_000_000),
        ("0.3200", "4.000004", 1_791_676_804_000_000_000),
    ]

    for consumer, values in [(first, expected[:2]), (second, expected)]:
        assert [
            (
                bar.bar_type,
                str(bar.open),
                str(bar.high),
                str(bar.low),
                str(bar.close),
                str(bar.volume),
                bar.ts_event,
                bar.ts_init,
            )
            for bar in consumer.events
        ] == [(BAR_TYPE, price, price, price, price, qty, ts, ts) for price, qty, ts in values]
        assert consumer.is_disposed()
    assert handle.state == NodeState.STOPPED
    assert [path for path, _, _ in requests] == [
        "/v1/exchanges",
        f"/v1/markets/POLYMARKET/{MARKET_ID}",
        endpoint,
        endpoint,
        endpoint,
    ]


class PollingConsumer(DataActor):
    """
    Observe the data engine's quote, interval-book, and internal-bar routes.
    """

    def __init__(self, config: DataActorConfig) -> None:
        """
        Retain a distinct actor identity and its observed events.
        """
        super().__init__(config)
        self.events = []

    def on_start(self) -> None:
        """
        Subscribe through the data engine to the selected feed.
        """
        if self.feed == "book":
            self.subscribe_book_at_interval(
                INSTRUMENT_ID,
                BookType.L2_MBP,
                20,
                client_id=FINFEED_CLIENT_ID,
            )
        elif self.feed == "bars":
            self.subscribe_bars(BAR_TYPE, client_id=FINFEED_CLIENT_ID)
        else:
            self.subscribe_quotes(INSTRUMENT_ID, client_id=FINFEED_CLIENT_ID)

    def on_stop(self) -> None:
        """
        Release explicitly stopped quote and interval-book subscriptions.
        """
        if self.feed == "book":
            self.unsubscribe_book_at_interval(INSTRUMENT_ID, 20, client_id=FINFEED_CLIENT_ID)
        elif self.feed == "quotes":
            self.unsubscribe_quotes(INSTRUMENT_ID, client_id=FINFEED_CLIENT_ID)

    def on_book(self, book) -> None:
        """
        Copy each interval snapshot before its managed book changes.
        """
        self.events.append(
            (
                book.instrument_id,
                book.book_type,
                book.sequence,
                book.update_count,
                book.ts_last,
                list(book.bids_to_dict().items()),
                list(book.asks_to_dict().items()),
            ),
        )

    def on_quote(self, quote) -> None:
        """
        Record sampled quotes.
        """
        self.events.append(quote)

    def on_bar(self, bar) -> None:
        """
        Record bars aggregated by the data engine from FinFeed trades.
        """
        self.events.append(bar)


class PollingController(Controller):
    """
    Expose registered lifecycle commands to the test coordinator.
    """

    current = None

    def on_start(self) -> None:
        """
        Retain the registered controller for actor lifecycle commands.
        """
        type(self).current = self


@pytest.fixture
def polling_node() -> Iterator[tuple[LiveNode, Queue, list]]:
    """
    Connect the FinFeed factory to a peer with individually released poll responses.
    """
    observed = Queue()
    requests = []
    replies = []
    PollingController.current = None

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            requests.append(
                (self.path, self.headers.get("Authorization"), self.headers.get("X-Referrer")),
            )

            if self.path == "/v1/exchanges":
                body = (TEST_DATA / "exchanges.json").read_bytes()
            elif self.path.startswith("/v1/markets/"):
                body = (TEST_DATA / "polymarket-market.json").read_bytes()
            else:
                reply = Queue()
                replies.append(reply)
                observed.put((self.path, reply, monotonic_ns()))
                body = reply.get(timeout=10)
                if body is None:
                    return
            self.send_response(200)
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
    config = FinFeedDataClientConfig(
        api_key="node-test-key",
        base_url_http=f"http://127.0.0.1:{server.server_port}/v1/",
        instruments=[FinFeedInstrumentSelection("POLYMARKET", [MARKET_ID])],
        min_request_interval_ms=1,
        poll_interval_ms=100,
        poll_limit=3,
        attempt_budget=16,
        max_retries=0,
    )
    node = (
        LiveNode.builder("FINFEED-POLLING", TraderId("TESTER-001"), Environment.LIVE)
        .with_reconciliation(False)
        .with_timeout_connection(5)
        .with_timeout_portfolio(0)
        .with_delay_post_stop_secs(0)
        .with_delay_shutdown_secs(0)
        .with_controller(
            ImportableControllerConfig(
                controller_path=f"{__name__}:PollingController",
                config_path="nautilus_trader.config:DataActorConfig",
                config={},
            ),
        )
        .add_data_client(None, FinFeedDataClientFactory(), config)
        .build()
    )
    try:
        yield node, observed, requests
        assert [(auth, referrer) for _, auth, referrer in requests] == [
            ("node-test-key", "nautilus"),
        ] * len(requests)
    finally:
        node.dispose()

        for reply in replies:
            reply.put(None)
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
        PollingController.current = None


async def _wait_for(predicate: Callable[[], bool]) -> None:
    async with asyncio.timeout(5):
        while not predicate():
            await asyncio.sleep(0.01)


def test_failed_instrument_list_refresh_keeps_node_cache_ownership() -> None:
    """
    Keep token ownership until a successful list refresh publishes.
    """
    names = ["alpha_NO", "beta_NO", "delta_NO", "gamma_NO", "epsilon_NO"]
    alpha, beta, delta, gamma, epsilon = (
        InstrumentId.from_str(f"{name}.FINFEED_POLYMARKET") for name in names
    )

    def definition(name: str, token: str) -> bytes:
        market = json.loads((TEST_DATA / "polymarket-market.json").read_bytes())
        market["market_id"] = name
        tokens = json.loads(market["source_specific_data"]["Tokens"])
        tokens[1]["token_id"] = token
        market["source_specific_data"]["Tokens"] = json.dumps(tokens)
        return json.dumps(market).encode()

    replies = Queue()

    for status, body in [
        (200, (TEST_DATA / "exchanges.json").read_bytes()),
        (200, definition("alpha_NO", "101")),
        (200, definition("beta_NO", "102")),
        (200, definition("delta_NO", "104")),
        (200, definition("alpha_NO", "103")),
        (404, (TEST_DATA / "empty.json").read_bytes()),
        (200, definition("gamma_NO", "101")),
        (200, definition("delta_NO", "104")),
        (200, definition("alpha_NO", "103")),
        (200, definition("beta_NO", "105")),
        (200, definition("gamma_NO", "101")),
        (200, definition("epsilon_NO", "104")),
        (200, definition("delta_NO", "104")),
    ]:
        replies.put((status, body))
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            requests.append(
                (self.path, self.headers.get("Authorization"), self.headers.get("X-Referrer")),
            )
            status, body = replies.get(timeout=5)
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_args: object) -> None:
            pass

    class RefreshConsumer(DataActor):
        def __init__(self) -> None:
            super().__init__()
            self.instruments = []

        def on_instrument(self, instrument) -> None:
            self.instruments.append(instrument)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    async def exercise() -> None:
        config = FinFeedDataClientConfig(
            api_key="node-test-key",
            base_url_http=f"http://127.0.0.1:{server.server_port}/v1/",
            instruments=[FinFeedInstrumentSelection("POLYMARKET", names[:2])],
            min_request_interval_ms=1,
            max_retries=0,
        )
        node = (
            LiveNode.builder("FINFEED-REFRESH", TraderId("TESTER-001"), Environment.LIVE)
            .with_reconciliation(False)
            .with_timeout_connection(5)
            .with_timeout_portfolio(0)
            .with_delay_post_stop_secs(0)
            .with_delay_shutdown_secs(0)
            .add_data_client(None, FinFeedDataClientFactory(), config)
            .build()
        )
        consumer = RefreshConsumer()
        node.add_actor(consumer)
        cache = node.cache
        handle = node.handle()
        task = asyncio.create_task(node.run_async())
        try:
            await _wait_for(lambda: consumer.is_running() and cache.instrument(beta) is not None)
            original = [
                cache.instrument(instrument_id).to_dict() for instrument_id in [alpha, beta]
            ]
            consumer.request_instrument(delta, client_id=FINFEED_CLIENT_ID)
            await _wait_for(lambda: len(consumer.instruments) == 1)

            consumer.request_instruments(alpha.venue, client_id=FINFEED_CLIENT_ID)
            await _wait_for(lambda: len(requests) == 6)
            consumer.request_instrument(gamma, client_id=FINFEED_CLIENT_ID)
            await _wait_for(lambda: len(requests) == 7)
            consumer.request_instrument(delta, client_id=FINFEED_CLIENT_ID)
            await _wait_for(lambda: len(consumer.instruments) >= 2)
            assert [instrument.id for instrument in consumer.instruments] == [delta, delta]
            assert [
                cache.instrument(instrument_id).to_dict() for instrument_id in [alpha, beta]
            ] == original
            assert cache.instrument(gamma) is None

            consumer.request_instruments(alpha.venue, client_id=FINFEED_CLIENT_ID)
            await _wait_for(lambda: len(consumer.instruments) == 4)
            assert [instrument.id for instrument in consumer.instruments[2:]] == [alpha, beta]
            assert cache.instrument(alpha).info["token_id"] == "103"
            assert cache.instrument(beta).info["token_id"] == "105"
            assert cache.instrument(delta).info["token_id"] == "104"

            consumer.request_instrument(gamma, client_id=FINFEED_CLIENT_ID)
            await _wait_for(lambda: len(consumer.instruments) == 5)
            assert consumer.instruments[-1].id == gamma
            assert cache.instrument(gamma).info["token_id"] == "101"
            consumer.request_instrument(epsilon, client_id=FINFEED_CLIENT_ID)
            await _wait_for(lambda: len(requests) == 12)
            consumer.request_instrument(delta, client_id=FINFEED_CLIENT_ID)
            await _wait_for(lambda: len(consumer.instruments) >= 6)
            assert [instrument.id for instrument in consumer.instruments] == [
                delta,
                delta,
                alpha,
                beta,
                gamma,
                delta,
            ]
            assert cache.instrument(epsilon) is None
            assert {instrument.id for instrument in cache.instruments()} == {
                alpha,
                beta,
                delta,
                gamma,
            }
        finally:
            handle.stop()
            await asyncio.wait_for(task, timeout=10)
            node.dispose()

    try:
        asyncio.run(exercise())
        assert requests == [
            (path, "node-test-key", "nautilus")
            for path in [
                "/v1/exchanges",
                *[
                    f"/v1/markets/POLYMARKET/{name}"
                    for name in [
                        "alpha_NO",
                        "beta_NO",
                        "delta_NO",
                        "alpha_NO",
                        "beta_NO",
                        "gamma_NO",
                        "delta_NO",
                        "alpha_NO",
                        "beta_NO",
                        "gamma_NO",
                        "epsilon_NO",
                        "delta_NO",
                    ]
                ],
            ]
        ]
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


@pytest.mark.parametrize("family", ["quotes", "trades"])
def test_public_activity_history_preserves_every_field_and_ties(polling_node, family: str) -> None:
    """
    Deliver exact selected history through the factory and public actor callbacks.
    """
    node, observed, requests = polling_node
    start = datetime(2026, 10, 11, 0, 0, 1, tzinfo=UTC)
    end = datetime(2026, 10, 11, 0, 0, 2, tzinfo=UTC)

    class HistoryConsumer(DataActor):
        def __init__(self) -> None:
            super().__init__()
            self.records = None

        def on_start(self) -> None:
            getattr(self, f"request_{family}")(
                INSTRUMENT_ID,
                start=start,
                end=end,
                limit=3,
                client_id=FINFEED_CLIENT_ID,
            )

        def on_historical_quotes(self, quotes) -> None:
            self.records = quotes

        def on_historical_trades(self, trades) -> None:
            self.records = trades

    consumer = HistoryConsumer()
    node.add_actor(consumer)
    handle = node.handle()

    async def exercise() -> None:
        task = asyncio.create_task(node.run_async())
        try:
            path, reply, _ = await asyncio.to_thread(observed.get, True, 5)
            assert path == (
                f"/v1/activity/POLYMARKET/{MARKET_ID}/history/{family}?date=2026-10-11&limit=10000"
            )
            reply.put((TEST_DATA / f"client-{family}.json").read_bytes())
            await _wait_for(lambda: consumer.records is not None)
        finally:
            handle.stop()
            await asyncio.wait_for(task, timeout=10)

    asyncio.run(exercise())
    stamp = int(start.timestamp()) * 1_000_000_000
    assert [record.instrument_id for record in consumer.records] == [INSTRUMENT_ID] * 3

    if family == "quotes":
        assert [
            (
                record.bid_price.as_decimal(),
                record.ask_price.as_decimal(),
                record.bid_size.as_decimal(),
                record.ask_size.as_decimal(),
                record.ts_event,
                record.ts_init,
            )
            for record in consumer.records
        ] == [
            (
                Decimal(".2"),
                Decimal(".3"),
                Decimal("5.000005"),
                Decimal("6.000006"),
                stamp - 1_000_000_000,
                stamp,
            ),
            (
                Decimal(".3"),
                Decimal(".4"),
                Decimal("3.000003"),
                Decimal("4.000004"),
                stamp,
                stamp + 1_000_000_000,
            ),
            (
                Decimal(".31"),
                Decimal(".41"),
                Decimal("7.000007"),
                Decimal("8.000008"),
                stamp,
                stamp + 1_000_000_000,
            ),
        ]
    else:
        assert [
            (
                str(record.trade_id),
                record.price.as_decimal(),
                record.size.as_decimal(),
                record.aggressor_side,
                record.ts_event,
                record.ts_init,
            )
            for record in consumer.records
        ] == [
            (
                "trade-2",
                Decimal(".21"),
                Decimal("3.000003"),
                AggressorSide.NO_AGGRESSOR,
                stamp,
                stamp,
            ),
            (
                "trade-1",
                Decimal(".31"),
                Decimal("2.000002"),
                AggressorSide.NO_AGGRESSOR,
                stamp + 1_000_000_000,
                stamp + 1_000_000_000,
            ),
            (
                "trade-3",
                Decimal(".32"),
                Decimal("4.000004"),
                AggressorSide.NO_AGGRESSOR,
                stamp + 1_000_000_000,
                stamp + 1_000_000_000,
            ),
        ]
    assert len(requests) == 3
    assert handle.state == NodeState.STOPPED


@pytest.mark.parametrize(
    ("receive_rows", "incompatible", "second", "microsecond", "tie_rows"),
    [
        pytest.param([], False, 6, 0, [], id="missing"),
        pytest.param([], False, 6, 0, [9], id="snapshot-update"),
        pytest.param([], False, 6, 0, [8], id="update-snapshot"),
        pytest.param([], False, 6, 0, [8, 9], id="multiple-transitions"),
        pytest.param([7], False, 2, 3, [], id="within"),
        pytest.param([7], True, 2, 1, [], id="partial-after-end"),
        pytest.param([4, 5, 6, 7], True, 2, 1, [], id="all-after-end"),
        pytest.param([4], True, 2, 3, [], id="reversed"),
    ],
)
def test_historical_books_reach_actor_and_existing_book_consumer(  # noqa: C901 - One actor lifecycle verifies all clock cases
    polling_node,
    receive_rows: list[int],
    incompatible: bool,
    second: int,
    microsecond: int,
    tie_rows: list[int],
) -> None:
    """
    Preserve complete historical actor delivery or reject incompatible clocks.
    """
    node, observed, requests = polling_node
    rows = json.loads((TEST_DATA / "book-replay.json").read_bytes())
    for index in receive_rows:
        rows[index]["time_coinapi"] = "2026-10-11T00:00:02.0000030Z"

    for index in tie_rows:
        rows[index]["time_exchange"] = rows[index - 1]["time_exchange"]
    source = json.dumps(rows).encode()
    start = datetime(2026, 10, 11, 0, 0, 1, 1, tzinfo=UTC)
    end = datetime(2026, 10, 11, 0, 0, second, microsecond, tzinfo=UTC)
    count = 5 if second == 6 else 2

    class HistoricalConsumer(DataActor):
        """
        Retain complete historical responses and replay completed delta batches.
        """

        def __init__(self) -> None:
            super().__init__()
            self.ids = []
            self.ladders = []
            self.groups = []
            self.deltas = None
            self.depths = None
            self.point = None

        def on_start(self) -> None:
            self.ids.append(
                self.request_book_deltas(
                    INSTRUMENT_ID,
                    start=start,
                    end=end,
                    client_id=FINFEED_CLIENT_ID,
                ),
            )
            self.ids.append(
                self.request_book_depth(
                    INSTRUMENT_ID,
                    start=start,
                    end=end,
                    depth=1,
                    client_id=FINFEED_CLIENT_ID,
                ),
            )
            self.ids.append(
                self.request_book_snapshot(
                    INSTRUMENT_ID,
                    depth=1,
                    client_id=FINFEED_CLIENT_ID,
                    params={"end": "2026-10-11T00:00:02.5Z"},
                ),
            )

        def on_historical_book_deltas(self, deltas) -> None:
            self.deltas = deltas
            book = OrderBook(INSTRUMENT_ID, BookType.L2_MBP)
            group = []
            for delta in deltas:
                group.append(delta)
                book.apply_delta(delta)
                if RecordFlag.F_LAST.matches(delta.flags):
                    self.groups.append(group)
                    group = []
                    book.check_integrity()
                    self.ladders.append((book.bids_to_dict(), book.asks_to_dict()))

        def on_historical_book_depth(self, depths) -> None:
            self.depths = depths

        def on_book(self, book) -> None:
            self.point = book

    consumer = HistoricalConsumer()
    node.add_actor(consumer)
    handle = node.handle()

    async def exercise() -> None:
        task = asyncio.create_task(node.run_async())
        try:
            for _ in range(3):
                path, reply, _ = await asyncio.to_thread(observed.get, True, 5)
                assert path.startswith(f"/v1/orderbook/POLYMARKET/{MARKET_ID}/history?")
                reply.put(source)
            if incompatible:
                await _wait_for(lambda: consumer.point is not None)
                await asyncio.sleep(0.2)
            else:
                await _wait_for(
                    lambda: (
                        consumer.depths is not None
                        and consumer.point is not None
                        and len(consumer.ladders) == count
                    ),
                )
        finally:
            handle.stop()
            await asyncio.wait_for(task, timeout=10)

    try:
        asyncio.run(exercise())
        assert handle.state == NodeState.STOPPED
        assert len(set(consumer.ids)) == 3
        expected = [
            (
                {Decimal(".4"): Decimal(2), Decimal(".3"): Decimal(5)},
                {Decimal(".6"): Decimal(3), Decimal(".7"): Decimal(7)},
            ),
            (
                {Decimal(".4"): Decimal("5.25"), Decimal(".3"): Decimal(5)},
                {Decimal(".65"): Decimal(8), Decimal(".7"): Decimal(7)},
            ),
            ({}, {Decimal(".8"): Decimal(9)}),
            ({}, {}),
            ({Decimal(".2"): Decimal(6)}, {Decimal(".9"): Decimal(10)}),
        ]

        if incompatible:
            assert consumer.deltas is None
            assert consumer.depths is None
            assert consumer.ladders == []
        else:
            assert consumer.ladders == expected[:count]
            assert [delta for group in consumer.groups for delta in group] == consumer.deltas
            for group in consumer.groups:
                snapshot = RecordFlag.F_SNAPSHOT.matches(group[0].flags)
                assert all(
                    RecordFlag.F_SNAPSHOT.matches(delta.flags) == snapshot for delta in group
                )

                if snapshot:
                    assert group[0].action == BookAction.CLEAR
                    assert all(delta.action == BookAction.ADD for delta in group[1:])
                else:
                    assert all(delta.action != BookAction.CLEAR for delta in group)
            assert len(consumer.depths) == count
            assert [(depth.ts_event, depth.ts_init) for depth in consumer.depths] == [
                (group[-1].ts_event, group[-1].ts_init) for group in consumer.groups
            ]
            assert [
                (order.price.as_decimal(), order.size.as_decimal())
                for order in consumer.depths[1].bids
            ] == [(Decimal(".4"), Decimal("5.25"))]
            assert [
                (order.price.as_decimal(), order.size.as_decimal())
                for order in consumer.depths[1].asks
            ] == [(Decimal(".65"), Decimal(8))]

            if receive_rows:
                assert consumer.deltas[-1].ts_init == consumer.deltas[-1].ts_event + 2900
                assert consumer.depths[-1].ts_init == consumer.deltas[-1].ts_init
        point_ladder = expected[1]
        if 8 in tie_rows:
            point_ladder = expected[3] if 9 in tie_rows else expected[2]
        assert consumer.point.bids_to_dict() == dict(list(point_ladder[0].items())[:1])
        assert consumer.point.asks_to_dict() == dict(list(point_ladder[1].items())[:1])
        assert len(requests) == 5
        assert all(
            auth == "node-test-key" and referrer == "nautilus" for _, auth, referrer in requests
        )
    finally:
        node.dispose()


@pytest.mark.parametrize(
    ("suffix", "period", "start", "end"),
    [
        ("1-MONTH", "1MTH", datetime(2024, 1, 1, tzinfo=UTC), datetime(2024, 2, 1, tzinfo=UTC)),
        ("3-MONTH", "3MTH", datetime(2023, 12, 1, tzinfo=UTC), datetime(2024, 3, 1, tzinfo=UTC)),
        ("1-YEAR", "1YRS", datetime(2024, 1, 1, tzinfo=UTC), datetime(2025, 1, 1, tzinfo=UTC)),
    ],
)
def test_public_bar_requests_return_complete_calendar_periods(
    polling_node,
    suffix: str,
    period: str,
    start: datetime,
    end: datetime,
) -> None:
    """
    Return complete long-period bars through the public Python actor request.
    """
    node, observed, requests = polling_node
    bar_type = BarType.from_str(f"{INSTRUMENT_ID}-{suffix}-LAST-EXTERNAL")
    row = json.loads((TEST_DATA / "client-bars.json").read_bytes())[0]
    row.update(
        time_period_start=start.isoformat(),
        time_period_end=end.isoformat(),
        time_open=start.isoformat(),
        time_close=end.isoformat(),
        volume_traded="3601.4359029999996",
    )

    class CalendarConsumer(DataActor):
        def __init__(self) -> None:
            super().__init__()
            self.bars = None

        def on_start(self) -> None:
            self.request_bars(
                bar_type,
                start=start,
                end=end,
                limit=1,
                client_id=FINFEED_CLIENT_ID,
            )

        def on_historical_bars(self, bars) -> None:
            self.bars = bars

    consumer = CalendarConsumer()
    node.add_actor(consumer)
    handle = node.handle()

    async def exercise() -> None:
        task = asyncio.create_task(node.run_async())
        try:
            path, reply, _ = await asyncio.to_thread(observed.get, True, 5)
            assert path == "/v1/ohlcv/periods"
            reply.put((TEST_DATA / "periods.json").read_bytes())
            path, reply, _ = await asyncio.to_thread(observed.get, True, 5)
            assert path.startswith(f"/v1/ohlcv/POLYMARKET/{MARKET_ID}/history?period_id={period}&")
            reply.put(json.dumps([row]).encode())
            await _wait_for(lambda: consumer.bars is not None)
        finally:
            handle.stop()
            await asyncio.wait_for(task, timeout=10)

    asyncio.run(exercise())
    [bar] = consumer.bars
    assert bar.bar_type == bar_type
    assert (str(bar.open), str(bar.high), str(bar.low), str(bar.close), str(bar.volume)) == (
        "0.2000",
        "0.4000",
        "0.1000",
        "0.3000",
        "3601.435903",
    )
    expected_end = int(end.timestamp()) * 1_000_000_000
    assert (bar.ts_event, bar.ts_init) == (expected_end, expected_end)
    assert len(requests) == 4
    assert handle.state == NodeState.STOPPED


@pytest.mark.parametrize(
    ("missing_response", "default_window"),
    [(False, False), (True, False), (False, True)],
)
def test_historical_book_example_replays_or_requires_response(
    polling_node,
    monkeypatch,
    missing_response,
    default_window,
) -> None:
    """
    Replay exact books through the example or time out without a response.
    """
    example = runpy.run_path(str(TEST_DATA.parents[3] / "examples/live/finfeed/historical_book.py"))
    actor_type = example["HistoricalBook"]
    state = actor_type.on_start.__globals__
    monkeypatch.setitem(state, "INSTRUMENT_ID", INSTRUMENT_ID)
    rows = json.loads((TEST_DATA / "book-replay.json").read_bytes())
    date = "2026-10-08" if default_window else "2026-10-11"

    if default_window:
        seed_time = rows[0]["time_exchange"]
        for row in rows:
            prefix = "00:00:" if row["time_exchange"] == seed_time else "00:01:"
            row["time_exchange"] = row["time_exchange"].replace(
                "2026-10-11T00:00:",
                f"{date}T{prefix}",
            )
    else:
        monkeypatch.setitem(state, "START", datetime(2026, 10, 11, 0, 0, 1, 1, tzinfo=UTC))
        monkeypatch.setitem(state, "END", datetime(2026, 10, 11, 0, 0, 6, tzinfo=UTC))
    actor = actor_type()
    node, observed, requests = polling_node
    node.add_actor(actor)
    handle = node.handle()

    async def exercise() -> None:
        task = asyncio.create_task(node.run_async())
        try:
            path, reply, _ = await asyncio.to_thread(observed.get, True, 5)
            assert path == (
                f"/v1/orderbook/POLYMARKET/{MARKET_ID}/history?"
                f"time_start={date}T00%3A00%3A00.0000000Z"
                f"&time_end={date}T01%3A00%3A00.0000000Z&limit=10000"
            )

            if missing_response:
                with pytest.raises(TimeoutError):
                    await asyncio.wait_for(actor.received.wait(), timeout=0.1)
            else:
                reply.put(json.dumps(rows).encode())
                await asyncio.wait_for(actor.received.wait(), timeout=5)
        finally:
            handle.stop()
            await asyncio.wait_for(task, timeout=10)

    asyncio.run(exercise())
    assert handle.state == NodeState.STOPPED
    assert len(requests) == 3
    assert actor.batches == (0 if missing_response else 5)
    assert actor.received.is_set() == (not missing_response)
    assert actor.book.bids_to_dict() == ({} if missing_response else {Decimal(".2"): Decimal(6)})
    assert actor.book.asks_to_dict() == ({} if missing_response else {Decimal(".9"): Decimal(10)})


def test_historical_book_example_uses_bounded_controls(monkeypatch) -> None:
    """
    Check native controls before building a node or network resource.
    """
    example = runpy.run_path(str(TEST_DATA.parents[3] / "examples/live/finfeed/historical_book.py"))
    configs: list[FinFeedDataClientConfig] = []

    def capture_config(**options: object) -> FinFeedDataClientConfig:
        config = FinFeedDataClientConfig(**options)
        configs.append(config)
        raise RuntimeError("Configuration captured before node construction")

    monkeypatch.setitem(example["main"].__globals__, "FinFeedDataClientConfig", capture_config)
    with pytest.raises(RuntimeError, match="Configuration captured before node construction"):
        asyncio.run(example["main"]())
    expected_controls = {
        "attempt_budget": 3,
        "http_timeout_secs": 60,
        "retry_operation_timeout_ms": 75_000,
        "retry_max_elapsed_ms": 90_000,
        "min_request_interval_ms": 10_000,
        "max_concurrent_requests": 1,
        "max_retries": 0,
        "connect_timeout_secs": 45,
    }
    assert len(configs) == 1
    assert {name: getattr(configs[0], name) for name in expected_controls} == expected_controls
    assert [(item.exchange_id, item.market_ids) for item in configs[0].instruments] == [
        ("POLYMARKET", [example["MARKET_ID"]]),
    ]
