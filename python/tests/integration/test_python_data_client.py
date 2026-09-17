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
Exercises Python data clients through the live engines.
"""

import asyncio
import gc
import subprocess
import sys
import threading
from datetime import UTC
from datetime import datetime
from decimal import Decimal
from pathlib import Path

import pytest

from nautilus_trader.common import Environment
from nautilus_trader.config import DataClientConfig
from nautilus_trader.config import ExecutionClientConfig
from nautilus_trader.config import ImportableFactoryConfig
from nautilus_trader.config import LiveNodeConfig
from nautilus_trader.infrastructure import RedisCacheConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveNodeBuilder
from nautilus_trader.live.clients import DataClientFactory
from nautilus_trader.live.clients import ExecutionClient
from nautilus_trader.live.clients import ExecutionClientFactory
from nautilus_trader.live.clients import MarketDataClient
from nautilus_trader.model import AccountBalance
from nautilus_trader.model import AccountId
from nautilus_trader.model import AccountType
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookOrder
from nautilus_trader.model import BookType
from nautilus_trader.model import ClientId
from nautilus_trader.model import Currency
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentCloseType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import InstrumentStatus
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import MarketStatusAction
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import Symbol
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import TradeTick
from nautilus_trader.model import Venue
from nautilus_trader.model import VenueOrderId
from nautilus_trader.trading import Strategy


INSTRUMENT_ID = InstrumentId.from_str("EUR/USD.PYTHON")


class Config(DataClientConfig):
    """
    Retain adapter fields alongside the native configuration.
    """

    def __init__(self, *, label, **_kwargs: object) -> None:
        """
        Retain the component inputs without starting asynchronous work.
        """
        self.label = label


class Client(MarketDataClient):
    """
    Implement the client hooks exercised by this scenario.
    """

    async def _connect(self):
        instrument = CurrencyPair(
            instrument_id=INSTRUMENT_ID,
            raw_symbol=Symbol("EURUSD"),
            base_currency=Currency.from_str("EUR"),
            quote_currency=Currency.from_str("USD"),
            price_precision=5,
            size_precision=0,
            price_increment=Price.from_str("0.00001"),
            size_increment=Quantity.from_str("1"),
            ts_event=11,
            ts_init=13,
        )
        self._handle_instrument(instrument)

    async def _disconnect(self):
        self.disconnected = True

    async def _subscribe_quotes(self, command):
        self.command = command
        self.callback_thread = threading.get_ident()
        self._handle_data(
            QuoteTick(
                instrument_id=command.instrument_id,
                bid_price=Price.from_str("1.12345"),
                ask_price=Price.from_str("1.12349"),
                bid_size=Quantity.from_str("17"),
                ask_size=Quantity.from_str("23"),
                ts_event=19,
                ts_init=29,
            ),
        )

    async def _unsubscribe_quotes(self, command):
        self.unsubscribed = command.instrument_id


class Factory(DataClientFactory):
    """
    Construct clients for the deterministic live scenario.
    """

    client = None

    @staticmethod
    def create(*, name, config, cache, clock) -> object:
        """
        Construct a fresh client for the supplied node context.
        """
        Factory.client = Client(
            name=name,
            config=config,
            cache=cache,
            clock=clock,
            venue=Venue("PYTHON"),
        )
        return Factory.client


class Consumer(Strategy):
    """
    Implement the client hooks exercised by this scenario.
    """

    def __init__(self) -> None:
        """
        Retain the component inputs without starting asynchronous work.
        """
        super().__init__()
        self.quotes = []

    def on_start(self) -> None:
        """
        Subscribe to quotes after the live node starts.
        """
        self.subscribe_quotes(INSTRUMENT_ID)

    def on_quote(self, quote) -> None:
        """
        Record the quote and request coordinated shutdown.
        """
        self.quotes.append(quote)
        self.shutdown_system("Quote received")


def build_node(factory=Factory, config=None, *, timeout_connection=2) -> object:
    """
    Build a data-only node using the supplied factory and config.
    """
    return (
        LiveNode.builder("PYTHON", TraderId("PYTHON-001"), Environment.SANDBOX)
        .with_reconciliation(False)
        .with_timeout_connection(timeout_connection)
        .with_timeout_portfolio(0)
        .with_delay_post_stop_secs(0)
        .with_timeout_disconnection_secs(1)
        .add_data_client(
            "PYTHON",
            factory,
            config if config is not None else Config(label="retained-field"),
        )
        .build()
    )


@pytest.mark.parametrize("launch", ["owned", "hosted"])
def test_custom_client_rejects_cache_database_in_both_launch_modes(launch) -> None:
    """
    Reject blocking cache backing before custom clients start on either loop.
    """
    node = (
        LiveNode.builder("PYTHON", TraderId("PYTHON-001"), Environment.SANDBOX)
        .with_cache_database_factory(RedisCacheConfig())
        .add_data_client("PYTHON", Factory, Config(label="database"))
        .build()
    )

    async def hosted() -> None:
        await node.run_async()

    def run() -> None:
        if launch == "owned":
            node.run()
        else:
            asyncio.run(hosted())

    try:
        with pytest.raises(
            RuntimeError,
            match="custom Python clients require a node without a cache database",
        ):
            run()
    finally:
        node.dispose()


@pytest.mark.parametrize("cleanup", ["dispose", "drop"])
@pytest.mark.parametrize("retain_builder", [False, True])
def test_unused_node_releases_adapter_cache(cleanup, retain_builder) -> None:
    """
    Release cache views even when a consumed builder remains reachable.
    """
    builder = LiveNode.builder(
        "PYTHON",
        TraderId("PYTHON-001"),
        Environment.SANDBOX,
    ).add_data_client("PYTHON", Factory, Config(label="retained-field"))
    node = builder.build()
    if not retain_builder:
        del builder
    cache = Factory.client.cache
    assert cache.instrument(INSTRUMENT_ID) is None

    if cleanup == "dispose":
        node.dispose()
    else:
        del node
        gc.collect()

    with pytest.raises(RuntimeError, match="disposed"):
        cache.instrument(INSTRUMENT_ID)


def test_factory_cannot_reuse_another_nodes_client() -> None:
    """
    Factory cannot reuse another nodes client.
    """
    node = build_node()
    client = Factory.client

    class ReusingFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            return client

    try:
        with pytest.raises(RuntimeError, match="owning factory cache view"):
            build_node(ReusingFactory, config=client.config)
    finally:
        node.dispose()


@pytest.mark.parametrize("registration", ["named", "instance", "importable", "direct_importable"])
def test_configuration_registration_uses_custom_factory_path(registration) -> None:
    """
    Configuration registration uses custom factory path.
    """
    original = Config(label="configured-client")
    descriptor = ImportableFactoryConfig(f"{__name__}:Factory")

    if registration == "instance":
        builder = LiveNode.builder("CONFIG", TraderId("CONFIG-001"), Environment.SANDBOX)
        builder.add_data_client("PYTHON", Factory(), original)
    elif registration == "direct_importable":
        builder = LiveNode.builder("CONFIG", TraderId("CONFIG-001"), Environment.SANDBOX)
        builder.add_data_client("PYTHON", descriptor, original.to_importable())
    else:
        client_config = original if registration == "named" else original.to_importable(descriptor)
        config = LiveNodeConfig(
            environment=Environment.SANDBOX,
            trader_id=TraderId("CONFIG-001"),
            data_clients={"PYTHON": client_config},
        )
        assert config.data_clients["PYTHON"] is client_config
        builder = LiveNodeBuilder.from_config(
            "CONFIG",
            config,
            data_factories={"PYTHON": Factory} if registration == "named" else None,
        )
    node = builder.with_reconciliation(False).build()
    try:
        assert Factory.client.config.label == "configured-client"
        if registration in ("named", "instance"):
            assert Factory.client.config is original
        assert Factory.client.client_id == ClientId("PYTHON")
        assert Factory.client.cache.instrument(INSTRUMENT_ID) is None
    finally:
        node.dispose()


def test_legacy_factory_loop_is_none_until_startup_binds_actual_loop() -> None:
    """
    Legacy factory loop is none until startup binds actual loop.
    """
    observed = []

    class LoopClient(Client):
        async def _connect(self):
            observed.append((self.loop, asyncio.get_running_loop()))
            await super()._connect()

    class LoopFactory(DataClientFactory):
        @staticmethod
        def create(*, loop, **kwargs: object) -> object:
            observed.append(loop)
            client = LoopClient(**kwargs, venue=Venue("PYTHON"))
            observed.append(client.loop)
            return client

    node = build_node(LoopFactory)
    assert observed == [None, None]
    node.add_strategy(Consumer())

    async def run():
        async with asyncio.timeout(5):
            await node.run_async()

    asyncio.run(run())
    assert observed[:2] == [None, None]
    assert observed[2][0] is observed[2][1]
    assert observed[2][0].is_closed()


def test_live_node_build_uses_configured_clients() -> None:
    """
    Live node build uses configured clients.
    """
    config = Config(label="direct-node-build")
    node = LiveNode.build(
        "CONFIG",
        LiveNodeConfig(environment=Environment.SANDBOX, data_clients={"PYTHON": config}),
        data_factories={"PYTHON": Factory},
    )
    try:
        assert Factory.client.config is config
        assert Factory.client.client_id == ClientId("PYTHON")
    finally:
        node.dispose()


def test_failed_build_retains_registrations_and_disposes_created_clients() -> None:
    """
    Failed build retains registrations and disposes created clients.
    """
    created = []
    attempts = 0

    class RetryFactory(DataClientFactory):
        @staticmethod
        def create(*, name, config, cache, clock) -> object:
            nonlocal attempts
            attempts += 1
            if attempts == 2:
                raise ValueError("Transient factory failure")
            client = Client(name=name, config=config, cache=cache, clock=clock, venue=Venue(name))
            created.append(client)
            return client

    builder = LiveNode.builder("RETRY", TraderId("RETRY-001"), Environment.SANDBOX)
    builder.add_data_client("FIRST", RetryFactory, Config(label="first"))
    builder.add_data_client("SECOND", RetryFactory, Config(label="second"))

    with pytest.raises(RuntimeError, match="Transient factory failure"):
        builder.build()

    assert attempts == 2
    assert len(created) == 1
    assert created[0]._runtime.complete
    with pytest.raises(RuntimeError, match="disposed"):
        created[0].cache.instrument(INSTRUMENT_ID)

    node = builder.build()
    try:
        assert attempts == 4
        assert {str(client.client_id): client.config.label for client in created[1:]} == {
            "FIRST": "first",
            "SECOND": "second",
        }
        assert all(client.cache.instrument(INSTRUMENT_ID) is None for client in created[1:])
    finally:
        node.dispose()

    for client in created[1:]:
        with pytest.raises(RuntimeError, match="disposed"):
            client.cache.instrument(INSTRUMENT_ID)


def test_owned_run_reports_resistant_cleanup_without_hanging() -> None:
    """
    Owned run reports resistant cleanup without hanging.
    """
    code = """
import asyncio
import runpy
import sys

source = runpy.run_path(sys.argv[1])
Client = source['Client']

class ResistantClient(Client):
    async def _connect(self):
        await super()._connect()
        self.create_task(self.resistant(), 'resistant')

    async def resistant(self):
        while True:
            try:
                await asyncio.sleep(60)
            except asyncio.CancelledError:
                pass

class Factory(source['DataClientFactory']):
    @staticmethod
    def create(**kwargs):
        return ResistantClient(**kwargs, venue=source['Venue']('PYTHON'))

node = source['build_node'](Factory)
node.add_strategy(source['Consumer']())
try:
    node.run()
except RuntimeError as e:
    assert 'disconnect' in str(e) or 'cleanup is incomplete' in str(e), str(e)
else:
    raise AssertionError('Incomplete cleanup must not report success')
"""
    result = subprocess.run(
        [sys.executable, "-I", "-c", code, str(Path(__file__).resolve())],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=8,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr


@pytest.mark.parametrize("launch", ["hosted", "owned"])
def test_python_factory_quote_reaches_engine_cache_and_strategy(launch) -> None:
    """
    Python factory quote reaches engine cache and strategy.
    """
    config = Config(label="retained-field")
    node = (
        LiveNode.builder("PYTHON", TraderId("PYTHON-001"), Environment.SANDBOX)
        .with_reconciliation(False)
        .with_timeout_connection(2)
        .with_timeout_portfolio(0)
        .with_delay_post_stop_secs(0)
        .with_timeout_disconnection_secs(2)
        .add_data_client("PYTHON", Factory, config)
        .build()
    )
    client = Factory.client
    assert client.config is config
    assert client.config.label == "retained-field"
    assert client.cache.instrument(INSTRUMENT_ID) is None
    for method in ("reset", "clear", "add_instrument", "add_order", "update_order", "cache_rc"):
        assert hasattr(client.cache, method) is False
    errors = []

    def foreign_read():
        try:
            client.cache.instrument(INSTRUMENT_ID)
        except RuntimeError as e:
            errors.append(str(e))

    thread = threading.Thread(target=foreign_read)
    thread.start()
    thread.join(timeout=2)
    assert thread.is_alive() is False
    assert errors == ["Client cache is disposed or accessed from a foreign thread"]
    consumer = Consumer()
    node.add_strategy(consumer)
    cache = node.cache

    async def hosted():
        async with asyncio.timeout(5):
            await node.run_async()

    if launch == "hosted":
        asyncio.run(hosted())
    else:
        node.run()

    expected = QuoteTick(
        instrument_id=INSTRUMENT_ID,
        bid_price=Price.from_str("1.12345"),
        ask_price=Price.from_str("1.12349"),
        bid_size=Quantity.from_str("17"),
        ask_size=Quantity.from_str("23"),
        ts_event=19,
        ts_init=29,
    )
    assert consumer.quotes == [expected]
    assert cache.quote(INSTRUMENT_ID) == expected
    assert cache.instrument(INSTRUMENT_ID).id == INSTRUMENT_ID
    assert client.callback_thread == threading.get_ident()
    assert client.disconnected is True
    with pytest.raises(RuntimeError, match="disposed"):
        client.cache.instrument(INSTRUMENT_ID)
    with pytest.raises(RuntimeError, match="not bound"):
        client._handle_data(expected)


@pytest.mark.parametrize(
    ("subscription", "callback", "expected"),
    [
        (
            "trades",
            "trade",
            TradeTick(
                INSTRUMENT_ID,
                Price.from_str("1.23456"),
                Quantity.from_str("37"),
                AggressorSide.BUY,
                TradeId("external-41"),
                43,
                47,
            ),
        ),
        (
            "mark_prices",
            "mark_price",
            MarkPriceUpdate(INSTRUMENT_ID, Price.from_str("1.23456"), 43, 47),
        ),
        (
            "index_prices",
            "index_price",
            IndexPriceUpdate(INSTRUMENT_ID, Price.from_str("1.34567"), 53, 59),
        ),
        (
            "funding_rates",
            "funding_rate",
            FundingRateUpdate(INSTRUMENT_ID, Decimal("0.000123456789"), 61, 67, interval=480),
        ),
        (
            "instrument_status",
            "instrument_status",
            InstrumentStatus(INSTRUMENT_ID, MarketStatusAction.TRADING, 71, 73),
        ),
        (
            "instrument_close",
            "instrument_close",
            InstrumentClose(
                INSTRUMENT_ID,
                Price.from_str("1.45678"),
                InstrumentCloseType.END_OF_SESSION,
                79,
                83,
            ),
        ),
    ],
)
@pytest.mark.asyncio
async def test_typed_subscription_and_data_reach_strategy(subscription, callback, expected) -> None:
    """
    Typed subscription and data reach strategy.
    """
    commands = []
    received = []
    params = {"tag": "distinct", "nested": {"sequence": 89}}

    async def subscribe(self, command):
        commands.append(command)
        self._handle_data(expected)

    async def unsubscribe(self, command):
        pass

    class TypedClient(Client):
        pass

    setattr(TypedClient, f"_subscribe_{subscription}", subscribe)
    setattr(TypedClient, f"_unsubscribe_{subscription}", unsubscribe)

    class TypedFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            return TypedClient(**kwargs, venue=Venue("PYTHON"))

    class TypedConsumer(Strategy):
        def on_start(self):
            getattr(self, f"subscribe_{subscription}")(
                INSTRUMENT_ID,
                client_id=ClientId("PYTHON"),
                params=params,
            )

    def on_data(self, data):
        received.append(data)
        self.shutdown_system("Typed data received")

    setattr(TypedConsumer, f"on_{callback}", on_data)
    node = build_node(TypedFactory)
    node.add_strategy(TypedConsumer())
    async with asyncio.timeout(5):
        await node.run_async()

    assert received == [expected]
    assert len(commands) == 1
    command = commands[0]
    assert command.instrument_id == INSTRUMENT_ID
    assert command.client_id == ClientId("PYTHON")
    assert command.venue == Venue("PYTHON")
    expected_params = {**params, "start_ns": None} if subscription == "trades" else params
    assert command.params == expected_params
    changed = command.params
    changed["nested"]["sequence"] = 97
    assert command.params == expected_params
    with pytest.raises(AttributeError):
        command.instrument_id = InstrumentId.from_str("GBP/USD.PYTHON")


@pytest.mark.asyncio
@pytest.mark.parametrize("empty", [False, True])
async def test_historical_quotes_preserve_request_and_response_correlation(empty) -> None:
    """
    Historical quotes preserve request and response correlation.
    """
    from nautilus_trader.live import QuotesResponse

    start = datetime(2024, 1, 2, 3, 4, 5, tzinfo=UTC)
    end = datetime(2024, 1, 2, 3, 4, 6, tzinfo=UTC)
    start_ns = 1704164645000000000
    end_ns = 1704164646000000000
    expected = (
        []
        if empty
        else [
            QuoteTick(
                INSTRUMENT_ID,
                Price.from_str("1.23456"),
                Price.from_str("1.23459"),
                Quantity.from_str("31"),
                Quantity.from_str("37"),
                start_ns + 41,
                start_ns + 43,
            ),
            QuoteTick(
                INSTRUMENT_ID,
                Price.from_str("1.34567"),
                Price.from_str("1.34569"),
                Quantity.from_str("47"),
                Quantity.from_str("53"),
                start_ns + 59,
                start_ns + 61,
            ),
        ]
    )
    requests = []
    responses = []
    received = []
    params = {"kind": "history", "sequence": 67}

    class HistoricalClient(Client):
        async def _request_quotes(self, request):
            requests.append(request)
            response = QuotesResponse(
                client_id=self.client_id,
                instrument_id=request.instrument_id,
                data=expected,
                correlation_id=request.request_id,
                ts_init=start_ns + 71,
                start=request.start_ns,
                end=request.end_ns,
                params=request.params,
            )
            responses.append(response)
            self._handle_response(response)

    class HistoricalFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            return HistoricalClient(**kwargs, venue=Venue("PYTHON"))

    class HistoricalConsumer(Strategy):
        def on_start(self):
            self.request_id = self.request_quotes(
                INSTRUMENT_ID,
                client_id=ClientId("PYTHON"),
                start=start,
                end=end,
                limit=2,
                params=params,
            )

        def on_historical_quotes(self, quotes):
            received.append(quotes)
            self.shutdown_system("Historical quotes received")

    node = build_node(HistoricalFactory)
    consumer = HistoricalConsumer()
    node.add_strategy(consumer)
    async with asyncio.timeout(5):
        await node.run_async()

    assert received == [expected]
    assert len(requests) == 1
    request = requests[0]
    assert str(request.request_id) == consumer.request_id
    assert request.client_id == ClientId("PYTHON")
    assert request.instrument_id == INSTRUMENT_ID
    assert request.start == start
    assert request.end == end
    assert request.start_ns == start_ns
    assert request.end_ns == end_ns
    assert request.limit == 2
    assert request.params == params
    response = responses[0]
    assert response.client_id == ClientId("PYTHON")
    assert response.instrument_id == INSTRUMENT_ID
    assert response.data == expected
    assert response.correlation_id == request.request_id
    assert response.ts_init == start_ns + 71
    assert response.start == start_ns
    assert response.end == end_ns
    assert response.params == params


@pytest.mark.asyncio
async def test_mutating_adapter_book_snapshot_cannot_change_core_cache() -> None:
    """
    Mutating adapter book snapshot cannot change core cache.
    """
    clients = []
    commands = []
    snapshots = []
    price = Price.from_str("1.23456")
    size = Quantity.from_str("107")
    delta = OrderBookDelta(
        INSTRUMENT_ID,
        BookAction.ADD,
        BookOrder(OrderSide.BUY, price, size, 101),
        128,
        103,
        109,
        113,
    )

    class BookClient(Client):
        async def _subscribe_book_deltas(self, command):
            commands.append(command)
            self._handle_data(OrderBookDeltas(INSTRUMENT_ID, [delta]))

    class BookFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            client = BookClient(**kwargs, venue=Venue("PYTHON"))
            clients.append(client)
            return client

    class BookConsumer(Strategy):
        def on_start(self):
            self.subscribe_book_deltas(INSTRUMENT_ID, BookType.L2_MBP, depth=5, managed=True)

        def on_book_deltas(self, _deltas):
            cache = clients[0].cache
            snapshot = cache.order_book(INSTRUMENT_ID)
            snapshots.append(snapshot.bids_to_dict())
            snapshot.clear(127, 131)
            snapshots.append(snapshot.bids_to_dict())
            snapshots.append(cache.order_book(INSTRUMENT_ID).bids_to_dict())
            self.shutdown_system("Snapshot isolation verified")

    node = build_node(BookFactory)
    node.add_strategy(BookConsumer())
    async with asyncio.timeout(5):
        await node.run_async()

    expected_bids = {Decimal("1.23456"): Decimal(107)}
    assert snapshots == [expected_bids, {}, expected_bids]
    assert node.cache.order_book(INSTRUMENT_ID).bids_to_dict() == expected_bids
    assert len(commands) == 1
    assert commands[0].book_type == BookType.L2_MBP
    assert commands[0].depth == 5
    assert commands[0].managed is True


@pytest.mark.parametrize("launch", ["hosted", "owned"])
def test_python_execution_client_reconciles_and_fills_through_core(launch) -> None:  # noqa: C901 - Keep the complete typed dispatch or engine scenario together.
    """
    Python execution client reconciles and fills through core.
    """
    clients = []
    requests = []
    commands = []
    fills = []
    account_id = AccountId("PYTHON-001")
    price = Price.from_str("1.23456")
    quantity = Quantity.from_str("1000")
    commission = Money.from_str("0.03 USD")

    class ConfigExecution(ExecutionClientConfig):
        def __init__(self, *, label) -> None:
            self.label = label

    class ClientExecution(ExecutionClient):
        async def _connect(self):
            self.generate_account_state(
                balances=[
                    AccountBalance(
                        Money.from_str("100000 USD"),
                        Money.from_str("0 USD"),
                        Money.from_str("100000 USD"),
                    ),
                    AccountBalance(
                        Money.from_str("0 EUR"),
                        Money.from_str("0 EUR"),
                        Money.from_str("0 EUR"),
                    ),
                ],
                margins=[],
                reported=True,
                ts_event=self.clock.timestamp_ns(),
                info={"source": "Python adapter"},
            )

        async def _disconnect(self):
            self.disconnected = True

        async def _generate_order_status_reports(self, command):
            requests.append(("orders", command))
            return []

        async def _generate_fill_reports(self, command):
            requests.append(("fills", command))
            return []

        async def _generate_position_status_reports(self, command):
            requests.append(("positions", command))
            return []

        async def _submit_order(self, command):
            commands.append(command)
            order = command.order
            self.generate_order_submitted(order)
            self.generate_order_accepted(
                order,
                VenueOrderId("external-137"),
                self.clock.timestamp_ns(),
            )
            self.generate_order_filled(
                order,
                VenueOrderId("external-137"),
                None,
                TradeId("fill-139"),
                quantity,
                price,
                Currency.from_str("USD"),
                commission,
                LiquiditySide.TAKER,
                self.clock.timestamp_ns(),
            )

    class FactoryExecution(ExecutionClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            client = ClientExecution(
                **kwargs,
                venue=Venue("PYTHON"),
                account_id=account_id,
                account_type=AccountType.CASH,
                oms_type=OmsType.NETTING,
            )
            clients.append(client)
            return client

    class ConsumerExecution(Strategy):
        def on_start(self):
            self.order = self.order_factory.limit(INSTRUMENT_ID, OrderSide.BUY, quantity, price)
            self.submit_order(self.order)

        def on_order_filled(self, event):
            fills.append(event)
            self.shutdown_system("Python execution fill received")

    config = ConfigExecution(label="retained-execution-config")
    node = (
        LiveNode.builder("PYTHON", TraderId("PYTHON-001"), Environment.SANDBOX)
        .with_timeout_connection(2)
        .with_timeout_portfolio(2)
        .with_timeout_reconciliation(2)
        .with_delay_post_stop_secs(0)
        .with_timeout_disconnection_secs(1)
        .add_data_client("PYTHON", Factory, Config(label="execution-instrument"))
        .add_exec_client("PYTHON", FactoryExecution, config)
        .build()
    )
    client = clients[0]
    consumer = ConsumerExecution()
    node.add_strategy(consumer)

    async def run():
        async with asyncio.timeout(5):
            await node.run_async()

    if launch == "hosted":
        asyncio.run(run())
    else:
        node.run()

    assert client.config is config
    assert client.config.label == "retained-execution-config"
    assert client.disconnected is True
    assert sorted(name for name, _ in requests) == ["fills", "orders", "positions"]
    assert len(commands) == 1
    command = commands[0]
    assert command.trader_id == TraderId("PYTHON-001")
    assert command.strategy_id == consumer.strategy_id
    assert command.instrument_id == INSTRUMENT_ID
    assert command.client_order_id == consumer.order.client_order_id
    assert command.order_init.quantity == quantity
    assert command.order_init.price == price
    assert len(fills) == 1
    fill = fills[0]
    assert fill.account_id == account_id
    assert fill.client_order_id == consumer.order.client_order_id
    assert fill.venue_order_id == VenueOrderId("external-137")
    assert fill.trade_id == TradeId("fill-139")
    assert fill.last_qty == quantity
    assert fill.last_px == price
    assert fill.commission == commission
    assert fill.liquidity_side == LiquiditySide.TAKER
    assert node.cache.order(consumer.order.client_order_id).status == OrderStatus.FILLED


@pytest.mark.asyncio
@pytest.mark.parametrize("kind", ["trades", "funding_rates", "bars", "book_deltas", "book_depth"])
@pytest.mark.parametrize("empty", [False, True])
async def test_historical_data_families_preserve_payload_and_correlation(
    kind,
    empty,
    adapter_payloads,
) -> None:
    """
    Every historical sequence traverses the request bridge and core response callback.
    """
    from nautilus_trader import live

    start_ns, bar_type, values = adapter_payloads
    end_ns = start_ns + 1000000000
    start = datetime.fromtimestamp(start_ns // 1000000000, UTC)
    end = datetime.fromtimestamp(end_ns // 1000000000, UTC)
    response_type = {
        "trades": live.TradesResponse,
        "funding_rates": live.FundingRatesResponse,
        "bars": live.BarsResponse,
        "book_deltas": live.BookDeltasResponse,
        "book_depth": live.BookDepthResponse,
    }[kind]
    expected = [] if empty else [values[kind]]
    requests = []
    received = []
    responses = []
    params = {"source": "archive", "nested": {"page": 107}}

    class HistoricalClient(Client):
        pass

    async def request(self, command):
        requests.append(command)
        identity = {"bar_type": bar_type} if kind == "bars" else {"instrument_id": INSTRUMENT_ID}
        response = response_type(
            client_id=self.client_id,
            data=expected,
            correlation_id=command.request_id,
            ts_init=start_ns + 109,
            start=command.start_ns,
            end=command.end_ns,
            params=command.params,
            **identity,
        )
        responses.append(response)
        self._handle_response(response)

    setattr(HistoricalClient, f"_request_{kind}", request)

    class HistoricalFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            return HistoricalClient(**kwargs, venue=Venue("PYTHON"))

    class HistoricalConsumer(Strategy):
        def on_start(self):
            options = {"depth": 10} if kind == "book_depth" else {}
            self.request_id = getattr(self, f"request_{kind}")(
                bar_type if kind == "bars" else INSTRUMENT_ID,
                client_id=ClientId("PYTHON"),
                start=start,
                end=end,
                limit=2,
                params=params,
                **options,
            )

    def receive(self, data):
        received.append(data)
        self.shutdown_system("Historical family received")

    setattr(HistoricalConsumer, f"on_historical_{kind}", receive)
    consumer = HistoricalConsumer()
    node = build_node(HistoricalFactory)
    node.add_strategy(consumer)
    async with asyncio.timeout(5):
        await node.run_async()

    assert received == [expected]
    assert len(requests) == 1
    command = requests[0]
    assert str(command.request_id) == consumer.request_id
    assert command.client_id == ClientId("PYTHON")
    assert command.start_ns == start_ns
    assert command.end_ns == end_ns
    assert command.start == start
    assert command.end == end
    assert command.limit == 2
    assert command.params == params
    if kind == "bars":
        assert command.bar_type == bar_type
    else:
        assert command.instrument_id == INSTRUMENT_ID
    if kind == "book_depth":
        assert command.depth == 10
    response = responses[0]
    assert response.client_id == ClientId("PYTHON")
    assert response.correlation_id == command.request_id
    assert response.ts_init == start_ns + 109
    assert response.start == start_ns
    assert response.end == end_ns
    assert response.params == params
    assert response.data == expected


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "kind",
    [
        "data",
        "instruments",
        "instrument",
        "book_deltas",
        "book_depth10",
        "quotes",
        "trades",
        "mark_prices",
        "index_prices",
        "funding_rates",
        "bars",
        "instrument_status",
        "instrument_close",
        "option_greeks",
    ],
)
async def test_all_subscription_families_preserve_subscribe_and_unsubscribe_fields(kind) -> None:  # noqa: C901 - Keep typed family dispatch and field assertions together.
    """
    Each subscription family forwards both operations with owned command metadata.
    """
    from nautilus_trader.model import BarType
    from nautilus_trader.model import DataType

    subscribed = asyncio.Event()
    unsubscribed = asyncio.Event()
    commands = []
    params = {"route": "custom", "nested": {"generation": 113}}
    bar_type = BarType.from_str("EUR/USD.PYTHON-1-MINUTE-LAST-EXTERNAL")
    data_type = DataType("AdapterSurface", metadata={"source": "external"})
    identity = {"data": data_type, "bars": bar_type, "instruments": Venue("PYTHON")}.get(
        kind,
        INSTRUMENT_ID,
    )
    extra = (
        {"book_type": BookType.L2_MBP, "managed": False}
        if kind in ("book_deltas", "book_depth10")
        else {}
    )

    if kind == "book_deltas":
        extra["depth"] = 7

    class TypedClient(Client):
        pass

    async def subscribe(self, command):
        commands.append(command)
        subscribed.set()

    async def unsubscribe(self, command):
        commands.append(command)
        unsubscribed.set()

    suffix = "" if kind == "data" else f"_{kind}"
    setattr(TypedClient, f"_subscribe{suffix}", subscribe)
    setattr(TypedClient, f"_unsubscribe{suffix}", unsubscribe)

    class TypedFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            return TypedClient(**kwargs, venue=Venue("PYTHON"))

    class Consumer(Strategy):
        def on_start(self):
            getattr(self, f"subscribe_{kind}")(
                identity,
                client_id=ClientId("PYTHON"),
                params=params,
                **extra,
            )

    node = build_node(TypedFactory)
    consumer = Consumer()
    node.add_strategy(consumer)
    handle = node.handle()
    task = asyncio.create_task(node.run_async())
    try:
        async with asyncio.timeout(5):
            await subscribed.wait()
            getattr(consumer, f"unsubscribe_{kind}")(
                identity,
                client_id=ClientId("PYTHON"),
                params=params,
            )
            await unsubscribed.wait()
    finally:
        handle.stop()
        async with asyncio.timeout(5):
            await task

    assert len(commands) == 2
    assert commands[0].command_id != commands[1].command_id
    for index, command in enumerate(commands):
        assert command.client_id == ClientId("PYTHON")
        assert command.venue == (None if kind == "data" else Venue("PYTHON"))
        expected_params = (
            {**params, "start_ns": None}
            if kind in ("data", "quotes", "trades", "bars") and index == 0
            else params
        )
        assert command.params == expected_params
        snapshot = command.params
        snapshot["nested"]["generation"] = 127
        assert command.params == expected_params
        if kind == "data":
            assert command.data_type == data_type
        elif kind == "bars":
            assert command.bar_type == bar_type
        elif kind != "instruments":
            assert command.instrument_id == INSTRUMENT_ID
        with pytest.raises(AttributeError):
            command.ts_init = 131
    if kind in ("book_deltas", "book_depth10"):
        assert commands[0].book_type == BookType.L2_MBP
        assert commands[0].managed is False
        assert commands[0].depth == (7 if kind == "book_deltas" else 10)


@pytest.mark.parametrize(
    ("kind", "message"),
    [
        ("return_type", "Factory must return a DataClient"),
        ("identity", "Factory client identity does not match registration name"),
        ("config", "Client must retain the original factory config"),
        ("async_factory", "Factory create must be synchronous"),
    ],
)
def test_data_factory_rejects_invalid_client_contract_and_releases_cache(kind, message) -> None:
    """
    Rejected factories cannot leak their cache view or alter the registration contract.
    """
    caches = []

    class InvalidFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            caches.append(kwargs["cache"])

            if kind == "return_type":
                return object()
            client = Client(**kwargs, venue=Venue("PYTHON"))
            if kind == "identity":
                client.client_id = ClientId("OTHER")
            elif kind == "config":
                client.config = DataClientConfig()
            return client

    if kind == "async_factory":

        async def create(**kwargs: object) -> object:
            return Client(**kwargs, venue=Venue("PYTHON"))

        InvalidFactory.create = staticmethod(create)

    with pytest.raises(RuntimeError, match=message):
        build_node(InvalidFactory)

    for cache in caches:
        with pytest.raises(RuntimeError, match="disposed"):
            cache.instrument(INSTRUMENT_ID)


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("kind", "bounded"),
    [
        ("data", False),
        ("data", True),
        ("instrument", False),
        ("instruments", False),
        ("book_snapshot", False),
        ("book_snapshot", True),
    ],
)
async def test_custom_instrument_and_book_requests_reach_response_callbacks(kind, bounded) -> None:  # noqa: C901 - Keep typed family dispatch and field assertions together.
    """
    Non-sequence response families retain payload and routing through the native engine.
    """
    from nautilus_trader import live
    from nautilus_trader.model import CustomData
    from nautilus_trader.model import DataType
    from nautilus_trader.model import OrderBook
    from nautilus_trader.testkit.providers import TestInstrumentProvider

    instrument = TestInstrumentProvider.audusd_sim()
    data_type = DataType("HistoricalAdapterData", metadata={"source": "external"})
    custom = CustomData(data_type, instrument)
    book = OrderBook(instrument.id, BookType.L2_MBP)
    book.add(
        BookOrder(OrderSide.BUY, Price.from_str("0.71231"), Quantity.from_str("17"), 19),
        0,
        23,
        29,
    )
    book.add(
        BookOrder(OrderSide.SELL, Price.from_str("0.71239"), Quantity.from_str("31"), 37),
        0,
        41,
        43,
    )
    requests = []
    received = []
    responses = []
    params = {"archive": "definitions", "sequence": 137}
    identity = {"data": data_type, "instruments": Venue("SIM")}.get(kind, instrument.id)

    class HistoricalClient(Client):
        async def _connect(self):
            pass

    async def request(self, command):
        requests.append(command)
        shared = {
            "client_id": self.client_id,
            "correlation_id": command.request_id,
            "ts_init": 139,
            "params": command.params,
        }

        if kind == "data":
            response = live.CustomDataResponse(
                data_type=data_type,
                venue=Venue("SIM"),
                data=[custom],
                **shared,
            )
        elif kind == "instrument":
            response = live.InstrumentResponse(
                instrument_id=instrument.id,
                data=instrument,
                **shared,
            )
        elif kind == "instruments":
            response = live.InstrumentsResponse(venue=Venue("SIM"), data=[instrument], **shared)
        else:
            response = live.BookResponse(instrument_id=instrument.id, data=book, **shared)
        responses.append(response)
        self._handle_response(response)

    setattr(HistoricalClient, f"_request_{kind}", request)

    class HistoricalFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            return HistoricalClient(**kwargs, venue=Venue("SIM"))

    class Consumer(Strategy):
        def on_start(self):
            options = (
                (
                    {"limit": 17}
                    if kind == "data"
                    else {"depth": 7}
                    if kind == "book_snapshot"
                    else {}
                )
                if bounded
                else {}
            )
            self.request_id = getattr(self, f"request_{kind}")(
                identity,
                client_id=ClientId("PYTHON"),
                params=params,
                **options,
            )

    def receive(self, data):
        received.append(data)
        self.shutdown_system("Non-sequence response received")

    callback = (
        "on_historical_data"
        if kind == "data"
        else "on_book"
        if kind == "book_snapshot"
        else "on_instrument"
    )
    setattr(Consumer, callback, receive)
    consumer = Consumer()
    node = build_node(HistoricalFactory)
    node.add_strategy(consumer)
    async with asyncio.timeout(5):
        await node.run_async()

    assert len(requests) == 1
    command = requests[0]
    assert str(command.request_id) == consumer.request_id
    assert command.client_id == ClientId("PYTHON")
    assert command.params == params
    assert len(responses) == 1
    assert responses[0].correlation_id == command.request_id
    assert responses[0].ts_init == 139
    assert responses[0].params == params
    assert len(received) == 1

    if kind == "data":
        assert command.data_type == data_type
        assert command.limit == (17 if bounded else None)
        assert command.start is None
        assert command.end is None
        assert command.start_ns is None
        assert command.end_ns is None
        assert [value.data for value in received[0]] == [instrument]
        assert [value.data_type for value in received[0]] == [data_type]
    elif kind == "book_snapshot":
        assert command.depth == (7 if bounded else None)
        assert command.instrument_id == instrument.id
        assert received[0].best_bid_price() == Price.from_str("0.71231")
        assert received[0].best_ask_price() == Price.from_str("0.71239")
        assert received[0].best_bid_size() == Quantity.from_str("17")
        assert received[0].best_ask_size() == Quantity.from_str("31")
        assert received[0].instrument_id == instrument.id
    else:
        assert received == [instrument]


@pytest.fixture
def adapter_payloads() -> tuple[int, object, dict[str, object]]:
    """
    Provide distinct live and historical payload fields for conversion tests.
    """
    from nautilus_trader.model import Bar
    from nautilus_trader.model import BarType
    from nautilus_trader.model import OrderBookDepth

    start_ns = 1704164645000000000
    bar_type = BarType.from_str("EUR/USD.PYTHON-1-MINUTE-LAST-EXTERNAL")
    bid = BookOrder(OrderSide.BUY, Price.from_str("1.23451"), Quantity.from_str("17"), 31)
    ask = BookOrder(OrderSide.SELL, Price.from_str("1.23459"), Quantity.from_str("23"), 37)
    values = {
        "trades": TradeTick(
            INSTRUMENT_ID,
            Price.from_str("1.23457"),
            Quantity.from_str("41"),
            AggressorSide.SELL,
            TradeId("HIST-43"),
            start_ns + 47,
            start_ns + 53,
        ),
        "funding_rates": FundingRateUpdate(
            INSTRUMENT_ID,
            Decimal("0.000123456789"),
            start_ns + 59,
            start_ns + 61,
            interval=480,
        ),
        "bars": Bar(
            bar_type,
            Price.from_str("1.23451"),
            Price.from_str("1.23459"),
            Price.from_str("1.23449"),
            Price.from_str("1.23457"),
            Quantity.from_str("67"),
            start_ns + 71,
            start_ns + 73,
        ),
        "book_deltas": OrderBookDelta(
            INSTRUMENT_ID,
            BookAction.ADD,
            bid,
            128,
            79,
            start_ns + 83,
            start_ns + 89,
        ),
        "book_depth": OrderBookDepth(
            INSTRUMENT_ID,
            [bid] * 10,
            [ask] * 10,
            [2] * 10,
            [3] * 10,
            128,
            97,
            start_ns + 101,
            start_ns + 103,
        ),
    }
    return start_ns, bar_type, values


@pytest.mark.asyncio
@pytest.mark.parametrize("kind", ["bars", "book_depth10", "book_deltas", "option_greeks", "data"])
async def test_additional_live_payloads_reach_strategy_without_losing_fields(
    kind,
    adapter_payloads,
) -> None:
    """
    Live output conversion preserves every field of structured and custom data.
    """
    from nautilus_trader.model import CustomData
    from nautilus_trader.model import DataType
    from nautilus_trader.model import OptionGreeks

    _, bar_type, values = adapter_payloads
    data_type = DataType("LiveAdapterData", metadata={"source": "external"})
    payload = {
        "bars": values["bars"],
        "book_depth10": values["book_depth"],
        "book_deltas": values["book_deltas"],
        "option_greeks": OptionGreeks(
            INSTRUMENT_ID,
            0.1,
            0.2,
            0.3,
            -0.4,
            rho=0.5,
            mark_iv=0.6,
            bid_iv=0.7,
            ask_iv=0.8,
            underlying_price=123.4,
            open_interest=17.5,
            ts_event=227,
            ts_init=229,
        ),
        "data": CustomData(data_type, values["trades"]),
    }[kind]
    received = []

    class TypedClient(Client):
        pass

    async def subscribe(self, command):
        self._handle_data(payload)

    setattr(TypedClient, "_subscribe" if kind == "data" else f"_subscribe_{kind}", subscribe)

    class TypedFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            return TypedClient(**kwargs, venue=Venue("PYTHON"))

    class Consumer(Strategy):
        def on_start(self):
            identity = (
                bar_type if kind == "bars" else data_type if kind == "data" else INSTRUMENT_ID
            )
            kwargs = (
                {"book_type": BookType.L2_MBP} if kind in ("book_deltas", "book_depth10") else {}
            )
            getattr(self, f"subscribe_{kind}")(identity, client_id=ClientId("PYTHON"), **kwargs)

    def receive(self, data):
        received.append(data)
        self.shutdown_system("Additional live payload received")

    callback = {
        "bars": "on_bar",
        "book_depth10": "on_book_depth",
        "book_deltas": "on_book_deltas",
        "option_greeks": "on_option_greeks",
        "data": "on_data",
    }[kind]
    setattr(Consumer, callback, receive)
    node = build_node(TypedFactory)
    node.add_strategy(Consumer())
    async with asyncio.timeout(5):
        await node.run_async()

    assert len(received) == 1
    if kind == "book_deltas":
        assert received[0].instrument_id == INSTRUMENT_ID
        assert received[0].deltas == [payload]
    elif kind == "data":
        assert received[0].data_type == data_type
        assert received[0].data == values["trades"]
    elif kind == "option_greeks":
        for field in (
            "instrument_id",
            "delta",
            "gamma",
            "vega",
            "theta",
            "rho",
            "mark_iv",
            "bid_iv",
            "ask_iv",
            "underlying_price",
            "open_interest",
            "ts_event",
            "ts_init",
            "convention",
        ):
            assert getattr(received[0], field) == getattr(payload, field)
    else:
        assert received == [payload]


def test_factory_cannot_reclaim_registered_output_with_replaced_context() -> None:
    """
    Fresh config and cache references cannot make a claimed output reusable.
    """
    clients = []

    class ReusingFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            if not clients:
                clients.append(Client(**kwargs, venue=Venue("PYTHON")))
            else:
                clients[0].cache = kwargs["cache"]
                clients[0].config = kwargs["config"]
            return clients[0]

    node = build_node(ReusingFactory)
    try:
        with pytest.raises(RuntimeError, match="A client instance cannot be registered twice"):
            build_node(ReusingFactory)
    finally:
        node.dispose()

    with pytest.raises(RuntimeError, match="disposed"):
        clients[0].cache.instrument(INSTRUMENT_ID)


@pytest.mark.parametrize(
    ("method", "message"),
    [
        ("_handle_data", "Expected a Nautilus data object from the installed wheel"),
        ("_handle_response", "Expected a Nautilus data response from the installed wheel"),
    ],
)
def test_invalid_data_output_is_rejected_without_interrupting_valid_delivery(
    method,
    message,
) -> None:
    """
    Reject foreign payloads before enqueueing, then continue normal data delivery.
    """
    rejected = []

    class InvalidOutputClient(Client):
        async def _connect(self):
            with pytest.raises(TypeError) as error:
                getattr(self, method)(object())
            rejected.append(str(error.value))
            await super()._connect()

    class InvalidOutputFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> Client:
            return InvalidOutputClient(**kwargs, venue=Venue("PYTHON"))

    node = build_node(InvalidOutputFactory)
    consumer = Consumer()
    node.add_strategy(consumer)
    node.run()

    assert rejected == [message]
    expected = QuoteTick(
        INSTRUMENT_ID,
        Price.from_str("1.12345"),
        Price.from_str("1.12349"),
        Quantity.from_str("17"),
        Quantity.from_str("23"),
        19,
        29,
    )
    assert node.cache.quote(INSTRUMENT_ID) == expected
    assert consumer.quotes == [expected]


@pytest.mark.parametrize("operation", ["connect", "disconnect"])
@pytest.mark.parametrize("launch", ["owned", "hosted"])
def test_failed_lifecycle_releases_background_tasks_and_cache(operation, launch) -> None:
    """
    A failing adapter hook reports failure and still releases its owned resources.
    """
    calls = []
    clients = []

    class FailingClient(Client):
        async def background(self, started):
            started.set()
            try:
                await asyncio.Event().wait()
            finally:
                calls.append("background_closed")

        async def _connect(self):
            started = asyncio.Event()
            self.create_task(self.background(started), "background")
            await started.wait()

            if operation == "connect":
                raise RuntimeError("Adapter connection failed")
            await super()._connect()

        async def _disconnect(self):
            if operation == "disconnect":
                raise RuntimeError("Adapter disconnection failed")
            await super()._disconnect()

    class FailingFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> Client:
            client = FailingClient(**kwargs, venue=Venue("PYTHON"))
            clients.append(client)
            return client

    node = build_node(FailingFactory, timeout_connection=1)
    node.add_strategy(Consumer())

    async def hosted():
        async with asyncio.timeout(5):
            await node.run_async()

    def run():
        if launch == "owned":
            node.run()
        else:
            asyncio.run(hosted())

    message = (
        "readiness timeout while waiting for engine connections"
        if operation == "connect"
        else "Adapter disconnection failed"
    )
    with pytest.raises(RuntimeError, match=message):
        run()

    assert calls == ["background_closed"]
    assert clients[0]._runtime.complete is True
    # A failed disconnect must not report a successfully disconnected transport
    assert clients[0].is_connected is (operation == "disconnect")
    with pytest.raises(RuntimeError, match="Client cache is disposed"):
        clients[0].cache.instrument(INSTRUMENT_ID)


@pytest.mark.asyncio
@pytest.mark.parametrize("join_cleanup", [False, True])
async def test_node_rechecks_incomplete_cleanup_until_background_task_finishes(
    join_cleanup,
) -> None:
    """
    Repeated disposal reports pending cleanup until the same task actually finishes.
    """
    release = asyncio.Event()
    entered = asyncio.Event()
    cleaning = asyncio.Event()
    tasks = []
    clients = []

    class CleanupClient(Client):
        async def _connect(self):
            await super()._connect()
            tasks.append(self.create_task(self.background(), "cleanup"))
            await entered.wait()

        async def _subscribe_quotes(self, command):
            await super()._subscribe_quotes(command)

            if join_cleanup:
                await tasks[0]

        async def _disconnect(self):
            if join_cleanup:
                await tasks[0]

        async def background(self):
            entered.set()
            try:
                await asyncio.Future()
            finally:
                cleaning.set()
                await release.wait()

    class CleanupFactory(DataClientFactory):
        @staticmethod
        def create(**kwargs: object) -> object:
            client = CleanupClient(**kwargs, venue=Venue("PYTHON"))
            clients.append(client)
            return client

    node = build_node(CleanupFactory)
    node.add_strategy(Consumer())
    try:
        async with asyncio.timeout(5):
            with pytest.raises(RuntimeError, match=r"disconnect|cleanup is incomplete"):
                await node.run_async()
        await asyncio.sleep(0)
        await asyncio.sleep(0)
        assert cleaning.is_set() is True

        for _ in range(2):
            with pytest.raises(RuntimeError, match="Python client cleanup is incomplete: PYTHON"):
                node.dispose()
        pending = tasks[0].done(), clients[0]._runtime.complete
    finally:
        release.set()
        await asyncio.gather(*tasks, return_exceptions=True)
        await asyncio.sleep(0)
        node.dispose()

    assert pending == (False, False)
    assert clients[0]._runtime.complete is True
