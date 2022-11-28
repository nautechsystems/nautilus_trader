# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from collections import Counter
from functools import partial
from unittest.mock import patch

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
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


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
    instrument_provider = BetfairInstrumentProvider(client=client, logger=logger, filters={})

    # Load instruments
    market_ids = BetfairDataProvider.market_ids()
    catalog = {
        r["marketId"]: r
        for r in BetfairResponses.betting_list_market_catalogue()["result"]
        if r["marketId"] in market_ids
    }
    mock_load_markets_metadata.return_value = catalog
    t = loop.create_task(
        instrument_provider.load_all_async(market_filter={"market_id": market_ids}),
    )
    loop.run_until_complete(t)

    # Fill INSTRUMENTS global cache
    INSTRUMENTS.extend(instrument_provider.list_all())
    assert INSTRUMENTS


class TestBetfairDataClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()

        self.trader_id = TestIdStubs.trader_id()
        self.uuid = UUID4()
        self.venue = BETFAIR_VENUE

        # Setup logging
        self.logger = LiveLogger(loop=self.loop, clock=self.clock, level_stdout=LogLevel.ERROR)
        self._log = LoggerAdapter("TestBetfairExecutionClient", self.logger)

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestComponentStubs.cache()
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
            betfair_client=self.betfair_client,
        )
        # Add a subset of instruments
        instruments = [
            ins for ins in INSTRUMENTS if ins.market_id in BetfairDataProvider.market_ids()
        ]
        self.instrument_provider.add_bulk(instruments)

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

        self.msgbus.deregister(
            endpoint="DataEngine.execute",
            handler=self.data_engine.execute,
        )
        self.msgbus.register(
            endpoint="DataEngine.execute",
            handler=partial(handler, endpoint="execute"),
        )

        self.msgbus.deregister(
            endpoint="DataEngine.process",
            handler=self.data_engine.process,
        )
        self.msgbus.register(
            endpoint="DataEngine.process",
            handler=partial(handler, endpoint="process"),
        )

        self.msgbus.deregister(
            endpoint="DataEngine.response",
            handler=self.data_engine.response,
        )
        self.msgbus.register(
            endpoint="DataEngine.response",
            handler=partial(handler, endpoint="response"),
        )

    @pytest.mark.asyncio
    @patch("nautilus_trader.adapters.betfair.data.BetfairDataClient._post_connect_heartbeat")
    @patch("nautilus_trader.adapters.betfair.data.BetfairMarketStreamClient.connect")
    @patch("nautilus_trader.adapters.betfair.client.core.BetfairClient.connect")
    async def test_connect(
        self,
        mock_client_connect,
        mock_stream_connect,
        mock_post_connect_heartbeat,
    ):
        await self.client._connect()

    def test_subscriptions(self):
        self.client.subscribe_trade_ticks(TestIdStubs.betting_instrument_id())
        self.client.subscribe_instrument_status_updates(TestIdStubs.betting_instrument_id())
        self.client.subscribe_instrument_close_prices(TestIdStubs.betting_instrument_id())

    def test_market_heartbeat(self):
        self.client._on_market_update(BetfairStreaming.mcm_HEARTBEAT())

    def test_stream_latency(self):
        logs = []
        self.logger.register_sink(logs.append)
        self.client.start()
        self.client._on_market_update(BetfairStreaming.mcm_latency())
        warning, degrading, degraded = logs[2:]
        assert warning["level"] == "WRN"
        assert warning["msg"] == "Stream unhealthy, waiting for recover"
        assert degraded["msg"] == "DEGRADED."

    def test_stream_con_true(self):
        logs = []
        self.logger.register_sink(logs.append)
        self.client._on_market_update(BetfairStreaming.mcm_con_true())
        (warning,) = logs
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
        result = {
            float(order[0])
            for ob_snap in self.messages
            if isinstance(ob_snap, OrderBookSnapshot)
            for order in ob_snap.bids + ob_snap.asks
        }
        expected = {
            0.0010204,
            0.0076923,
            0.0217391,
            0.0238095,
            0.1724138,
            0.2173913,
            0.3676471,
            0.3937008,
            0.4587156,
            0.5555556,
        }
        assert result == expected

    def test_market_sub_image_no_market_def(self):
        self.client._on_market_update(BetfairStreaming.mcm_SUB_IMAGE_no_market_def())
        result = Counter([type(event).__name__ for event in self.messages])
        expected = Counter(
            {
                "InstrumentStatusUpdate": 270,
                "OrderBookSnapshot": 270,
                "InstrumentClosePrice": 22,
            },
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
        assert update_op.order.price == 0.212766

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
        provider = self.client.instrument_provider
        for mc in update[0]["mc"]:
            market_def = {**mc["marketDefinition"], "marketId": mc["id"]}
            instruments = make_instruments(market_definition=market_def, currency="GBP")
            provider.add_bulk(instruments)

        for u in update:
            self.client._on_market_update(u)
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
        self.client.request(req, self.uuid)
        await asyncio.sleep(0)
        resp = self.messages[0]
        assert len(resp.data.instruments) == 6800

    def test_orderbook_repr(self):
        self.client._on_market_update(BetfairStreaming.mcm_live_IMAGE())
        ob_snap = self.messages[14]
        ob = L2OrderBook(InstrumentId(Symbol("1"), BETFAIR_VENUE), 5, 5)
        ob.apply_snapshot(ob_snap)
        print(ob.pprint())
        assert ob.best_ask_price() == 0.5882353
        assert ob.best_bid_price() == 0.5847953

    def test_orderbook_updates(self):
        order_books = {}
        for raw_update in BetfairStreaming.market_updates():
            for update in on_market_update(
                update=raw_update,
                instrument_provider=self.client.instrument_provider,
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
            instrument_provider=self.client.instrument_provider,
            update=updates[0],
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
                instrument_provider=self.client.instrument_provider,
                update=update,
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
            instrument_provider=self.client.instrument_provider,
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
        assert ticker.last_traded_price == Price.from_str("0.3174603")
        assert ticker.traded_volume == Quantity.from_str("364.45")

    def test_betfair_orderbook(self):
        book = L2OrderBook(
            instrument_id=TestIdStubs.betting_instrument_id(),
            price_precision=2,
            size_precision=2,
        )
        for update in BetfairDataProvider.raw_market_updates():
            for message in on_market_update(
                instrument_provider=self.instrument_provider,
                update=update,
            ):
                try:
                    if isinstance(message, OrderBookSnapshot):
                        book.apply_snapshot(message)
                    elif isinstance(message, OrderBookDeltas):
                        book.apply_deltas(message)
                    elif isinstance(message, OrderBookDelta):
                        book.apply_delta(message)
                    elif isinstance(
                        message,
                        (Ticker, TradeTick, InstrumentStatusUpdate, InstrumentClosePrice),
                    ):
                        pass
                    else:
                        raise NotImplementedError(str(type(message)))
                    book.check_integrity()
                except Exception as e:
                    print(str(type(e)) + " " + str(e))
