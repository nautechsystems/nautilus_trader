# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
import sys
from collections import Counter
from functools import partial
from unittest.mock import patch
from uuid import uuid4

import pytest

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.data import InstrumentSearch
from nautilus_trader.adapters.betfair.data import on_market_update
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import make_instruments
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.data.venue import InstrumentClosePrice
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import InstrumentStatus
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.book import L2OrderBook
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit.stubs import TestStubs


pytestmark = pytest.mark.skipif(sys.version_info < (3, 8), reason="requires python3.8 or higher")


INSTRUMENTS = []


@pytest.fixture(scope="session", autouse=True)
@patch("nautilus_trader.adapters.betfair.providers.load_markets_metadata")
def instrument_list(mock_load_markets_metadata, loop: asyncio.AbstractEventLoop):
    """Prefill `INSTRUMENTS` cache for tests"""
    global INSTRUMENTS

    # Setup
    logger = LiveLogger(loop=loop, clock=LiveClock(), level_stdout=LogLevel.ERROR)
    client = BetfairTestStubs.betfair_client(loop=loop, logger=logger)
    logger = LiveLogger(loop=loop, clock=LiveClock(), level_stdout=LogLevel.DEBUG)
    instrument_provider = BetfairInstrumentProvider(client=client, logger=logger, market_filter={})

    # Load instruments
    catalog = {r["marketId"]: r for r in BetfairResponses.betting_list_market_catalogue()["result"]}
    mock_load_markets_metadata.return_value = catalog
    t = loop.create_task(instrument_provider.load_all_async())
    loop.run_until_complete(t)

    # Fill INSTRUMENTS global cache
    INSTRUMENTS.extend(instrument_provider.list_instruments())
    assert INSTRUMENTS


class TestBetfairDataClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()

        self.trader_id = TestStubs.trader_id()
        self.uuid = uuid4()
        self.venue = BETFAIR_VENUE
        self.account_id = AccountId(self.venue.value, "001")

        # Setup logging
        self.logger = LiveLogger(loop=self.loop, clock=self.clock, level_stdout=LogLevel.ERROR)
        self._log = LoggerAdapter("TestBetfairExecutionClient", self.logger)

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()
        self.cache.add_instrument(BetfairTestStubs.betting_instrument())

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.betfair_client = BetfairTestStubs.betfair_client(loop=self.loop, logger=self.logger)

        self.instrument_provider = BetfairTestStubs.instrument_provider(
            betfair_client=self.betfair_client
        )
        self.instrument_provider.add_instruments(INSTRUMENTS)

        self.client = BetfairDataClient(
            loop=self.loop,
            client=self.betfair_client,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            instrument_provider=self.instrument_provider,
            market_filter={},
        )

        self.data_engine.register_client(self.client)

        # Re-route exec engine messages through `handler`
        self.messages = []

        def handler(x, endpoint):
            self.messages.append(x)
            if endpoint == "execute":
                self.data_engine.execute(x)
            elif endpoint == "process":
                self.data_engine.process(x)
            elif endpoint == "response":
                self.data_engine.response(x)

        self.msgbus.deregister(endpoint="DataEngine.execute", handler=self.data_engine.execute)  # type: ignore
        self.msgbus.register(
            endpoint="DataEngine.execute", handler=partial(handler, endpoint="execute")  # type: ignore
        )

        self.msgbus.deregister(endpoint="DataEngine.process", handler=self.data_engine.process)  # type: ignore
        self.msgbus.register(
            endpoint="DataEngine.process", handler=partial(handler, endpoint="process")  # type: ignore
        )

        self.msgbus.deregister(endpoint="DataEngine.response", handler=self.data_engine.response)  # type: ignore
        self.msgbus.register(
            endpoint="DataEngine.response", handler=partial(handler, endpoint="response")  # type: ignore
        )

    def test_subscriptions(self):
        self.client.subscribe_trade_ticks(BetfairTestStubs.instrument_id())
        self.client.subscribe_instrument_status_updates(BetfairTestStubs.instrument_id())
        self.client.subscribe_instrument_close_prices(BetfairTestStubs.instrument_id())

    def test_market_heartbeat(self):
        self.client._on_market_update(BetfairStreaming.mcm_HEARTBEAT())

    def test_stream_latency(self):
        logs = []
        self.logger.register_sink(logs.append)
        self.client._on_market_update(BetfairStreaming.mcm_latency())
        warning, _ = logs
        assert warning["level"] == "WRN"
        assert warning["msg"] == "Stream unhealthy, waiting for recover"

    def test_stream_con_true(self):
        logs = []
        self.logger.register_sink(logs.append)
        self.client._on_market_update(BetfairStreaming.mcm_con_true())
        warning, _ = logs
        assert warning["level"] == "WRN"
        assert (
            warning["msg"]
            == "Conflated stream - consuming data too slow (data received is delayed)"
        )

    @pytest.mark.asyncio
    async def test_market_sub_image_market_def(self):
        update = BetfairStreaming.mcm_SUB_IMAGE()
        self.client._on_market_update(update)
        result = [type(event).__name__ for event in self.messages]
        expected = ["InstrumentStatusUpdate"] * 7 + ["OrderBookSnapshot"] * 7
        assert result == expected
        # Check prices are probabilities
        result = set(
            float(order[0])
            for ob_snap in self.messages
            if isinstance(ob_snap, OrderBookSnapshot)
            for order in ob_snap.bids + ob_snap.asks
        )
        expected = set(
            [
                0.02174,
                0.39370,
                0.36765,
                0.21739,
                0.00102,
                0.17241,
                0.00102,
                0.55556,
                0.45872,
                0.21739,
                0.00769,
                0.02381,
            ]
        )
        assert result == expected

    def test_market_sub_image_no_market_def(self):
        self.client._on_market_update(BetfairStreaming.mcm_SUB_IMAGE_no_market_def())
        result = Counter([type(event).__name__ for event in self.messages])
        expected = Counter(
            {
                "InstrumentStatusUpdate": 270,
                "OrderBookSnapshot": 270,
                "InstrumentClosePrice": 22,
            }
        )
        assert result == expected

    def test_market_resub_delta(self):
        self.client._on_market_update(BetfairStreaming.mcm_RESUB_DELTA())
        result = [type(event).__name__ for event in self.messages]
        expected = ["InstrumentStatusUpdate"] * 12 + ["OrderBookDeltas"] * 269
        assert result == expected

    def test_market_update(self):
        self.client._on_market_update(BetfairStreaming.mcm_UPDATE())
        result = [type(event).__name__ for event in self.messages]
        expected = ["OrderBookDeltas"] * 1
        assert result == expected
        result = [d.action for d in self.messages[0].deltas]
        expected = [BookAction.UPDATE, BookAction.DELETE]
        assert result == expected
        # Ensure order prices are coming through as probability
        update_op = self.messages[0].deltas[0]
        assert update_op.order.price == 0.21277

    def test_market_update_md(self):
        self.client._on_market_update(BetfairStreaming.mcm_UPDATE_md())
        result = [type(event).__name__ for event in self.messages]
        expected = ["InstrumentStatusUpdate"] * 2
        assert result == expected

    def test_market_update_live_image(self):
        self.client._on_market_update(BetfairStreaming.mcm_live_IMAGE())
        result = [type(event).__name__ for event in self.messages]
        expected = (
            ["OrderBookSnapshot"] + ["TradeTick"] * 13 + ["OrderBookSnapshot"] + ["TradeTick"] * 17
        )
        assert result == expected

    def test_market_update_live_update(self):
        self.client._on_market_update(BetfairStreaming.mcm_live_UPDATE())
        result = [type(event).__name__ for event in self.messages]
        expected = ["TradeTick", "OrderBookDeltas"]
        assert result == expected

    def test_market_bsp(self):
        # Setup
        update = BetfairStreaming.mcm_BSP()
        provider = self.client.instrument_provider()
        for mc in update[0]["mc"]:
            market_def = {**mc["marketDefinition"], "marketId": mc["id"]}
            instruments = make_instruments(market_definition=market_def, currency="GBP")
            provider.add_instruments(instruments)

        for update in update:
            self.client._on_market_update(update)
        result = Counter([type(event).__name__ for event in self.messages])
        expected = {
            "TradeTick": 95,
            "BSPOrderBookDelta": 30,
            "InstrumentStatusUpdate": 9,
            "OrderBookSnapshot": 8,
            "OrderBookDeltas": 2,
        }
        assert result == expected

    @pytest.mark.asyncio
    async def test_request_search_instruments(self):
        req = DataType(
            type=InstrumentSearch,
            metadata={"event_type_id": "7"},
        )
        self.client.request(req, UUID4(str(self.uuid)))
        await asyncio.sleep(0)
        resp = self.messages[0]
        assert len(resp.data.instruments) == 9416

    def test_orderbook_repr(self):
        self.client._on_market_update(BetfairStreaming.mcm_live_IMAGE())
        ob_snap = self.messages[14]
        ob = L2OrderBook(InstrumentId(Symbol("1"), BETFAIR_VENUE), 5, 5)
        ob.apply_snapshot(ob_snap)
        print(ob.pprint())
        assert ob.best_ask_price() == 0.58824
        assert ob.best_bid_price() == 0.58480

    def test_orderbook_updates(self):
        order_books = {}
        for raw_update in BetfairStreaming.market_updates():
            for update in on_market_update(
                update=raw_update,
                instrument_provider=self.client.instrument_provider(),
            ):
                if len(order_books) > 1 and update.instrument_id != list(order_books)[1]:
                    continue
                print(update)
                if isinstance(update, OrderBookSnapshot):
                    order_books[update.instrument_id] = L2OrderBook(
                        instrument_id=update.instrument_id,
                        price_precision=4,
                        size_precision=4,
                    )
                    order_books[update.instrument_id].apply_snapshot(update)
                elif isinstance(update, OrderBookDeltas):
                    order_books[update.instrument_id].apply_deltas(update)
                elif isinstance(update, TradeTick):
                    pass
                else:
                    raise KeyError

        book = order_books[list(order_books)[0]]
        expected = """bids       price   asks
--------  -------  ---------
          0.8621   [932.64]
          0.8547   [1275.83]
          0.8475   [151.96]
[147.79]  0.8403
[156.74]  0.8333
[11.19]   0.8197"""
        result = book.pprint()
        assert result == expected

    def test_instrument_opening_events(self):
        updates = BetfairDataProvider.raw_market_updates()
        messages = on_market_update(
            instrument_provider=self.client.instrument_provider(), update=updates[0]
        )
        assert len(messages) == 2
        assert (
            isinstance(messages[0], InstrumentStatusUpdate)
            and messages[0].status == InstrumentStatus.PRE_OPEN
        )
        assert (
            isinstance(messages[1], InstrumentStatusUpdate)
            and messages[0].status == InstrumentStatus.PRE_OPEN
        )

    def test_instrument_in_play_events(self):
        events = [
            msg
            for update in BetfairDataProvider.raw_market_updates()
            for msg in on_market_update(
                instrument_provider=self.client.instrument_provider(), update=update
            )
            if isinstance(msg, InstrumentStatusUpdate)
        ]
        assert len(events) == 14
        result = [ev.status for ev in events]
        expected = [
            InstrumentStatus.PRE_OPEN.value,
            InstrumentStatus.PRE_OPEN.value,
            InstrumentStatus.PRE_OPEN.value,
            InstrumentStatus.PRE_OPEN.value,
            InstrumentStatus.PRE_OPEN.value,
            InstrumentStatus.PRE_OPEN.value,
            InstrumentStatus.PAUSE.value,
            InstrumentStatus.PAUSE.value,
            InstrumentStatus.OPEN.value,
            InstrumentStatus.OPEN.value,
            InstrumentStatus.PAUSE.value,
            InstrumentStatus.PAUSE.value,
            InstrumentStatus.CLOSED.value,
            InstrumentStatus.CLOSED.value,
        ]
        assert result == expected

    def test_instrument_closing_events(self):
        updates = BetfairDataProvider.raw_market_updates()
        messages = on_market_update(
            instrument_provider=self.client.instrument_provider(),
            update=updates[-1],
        )
        assert len(messages) == 4
        assert (
            isinstance(messages[0], InstrumentStatusUpdate)
            and messages[0].status == InstrumentStatus.CLOSED
        )
        assert isinstance(messages[1], InstrumentClosePrice) and messages[1].close_price == 1.0000
        assert (
            isinstance(messages[1], InstrumentClosePrice)
            and messages[1].close_type == InstrumentCloseType.EXPIRED
        )
        assert (
            isinstance(messages[2], InstrumentStatusUpdate)
            and messages[2].status == InstrumentStatus.CLOSED
        )
        assert isinstance(messages[3], InstrumentClosePrice) and messages[3].close_price == 0.0
        assert (
            isinstance(messages[3], InstrumentClosePrice)
            and messages[3].close_type == InstrumentCloseType.EXPIRED
        )

    def test_betfair_ticker(self):
        self.client._on_market_update(BetfairStreaming.mcm_UPDATE_tv())
        ticker: BetfairTicker = self.messages[1]
        assert ticker.last_traded_price == Price.from_str("0.31746")
        assert ticker.traded_volume == Quantity.from_str("364.45")

    def test_betfair_orderbook(self):
        book = L2OrderBook(
            instrument_id=BetfairTestStubs.instrument_id(),
            price_precision=2,
            size_precision=2,
        )
        for update in BetfairDataProvider.raw_market_updates():
            for message in on_market_update(
                instrument_provider=self.instrument_provider, update=update
            ):
                try:
                    if isinstance(message, OrderBookSnapshot):
                        book.apply_snapshot(message)
                    elif isinstance(message, OrderBookDeltas):
                        book.apply_deltas(message)
                    elif isinstance(message, OrderBookDelta):
                        book.apply_delta(message)
                    elif isinstance(
                        message, (Ticker, TradeTick, InstrumentStatusUpdate, InstrumentClosePrice)
                    ):
                        pass
                    else:
                        raise NotImplementedError(str(type(message)))
                    book.check_integrity()
                except Exception as ex:
                    print(str(type(ex)) + " " + str(ex))
