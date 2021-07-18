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
import json
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.ccxt.data import CCXTDataClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.type import DataType
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.data.messages import DataRequest
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.message_bus import MessageBus
from nautilus_trader.trading.portfolio import Portfolio
from tests import TESTS_PACKAGE_ROOT
from tests.test_kit.mocks import ObjectStorer
from tests.test_kit.stubs import TestStubs


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/ccxt/responses/"

BINANCE = Venue("BINANCE")
BTCUSDT = InstrumentId(Symbol("BTC/USDT"), BINANCE)
ETHUSDT = InstrumentId(Symbol("ETH/USDT"), BINANCE)


# Monkey patch magic mock
# This allows the stubbing of calls to coroutines
MagicMock.__await__ = lambda x: async_magic().__await__()


# Dummy method for above
async def async_magic():
    return


class TestCCXTDataClient:
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.trader_id = TestStubs.trader_id()

        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        # Setup logging
        self.logger = LiveLogger(
            loop=self.loop,
            clock=self.clock,
        )

        self.msgbus = MessageBus(
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Setup mock CCXT exchange
        with open(TEST_PATH + "markets.json") as response:
            markets = json.load(response)

        with open(TEST_PATH + "currencies.json") as response:
            currencies = json.load(response)

        with open(TEST_PATH + "watch_order_book.json") as response:
            order_book = json.load(response)

        with open(TEST_PATH + "fetch_trades.json") as response:
            fetch_trades = json.load(response)

        with open(TEST_PATH + "watch_trades.json") as response:
            watch_trades = json.load(response)

        self.mock_ccxt = MagicMock()
        self.mock_ccxt.name = "Binance"
        self.mock_ccxt.precisionMode = 2
        self.mock_ccxt.markets = markets
        self.mock_ccxt.currencies = currencies
        self.mock_ccxt.watch_order_book = order_book
        self.mock_ccxt.watch_trades = watch_trades
        self.mock_ccxt.fetch_trades = fetch_trades

        self.client = CCXTDataClient(
            client=self.mock_ccxt,
            engine=self.data_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine.register_client(self.client)

    @pytest.mark.asyncio
    async def test_connect(self):
        # Arrange
        # Act
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        # Assert
        assert self.client.is_connected

        # Tear down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_disconnect(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        # Act
        self.client.disconnect()
        await asyncio.sleep(0.3)

        # Assert
        assert not self.client.is_connected

        # Tear down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_reset_when_not_connected_successfully_resets(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        self.data_engine.stop()
        await asyncio.sleep(0.3)  # Allow engine message queue to stop

        # Act
        self.client.reset()

        # Assert
        assert not self.client.is_connected

    @pytest.mark.asyncio
    async def test_reset_when_connected_does_not_reset(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        # Act
        self.client.reset()

        # Assert
        assert self.client.is_connected

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_dispose_when_not_connected_does_not_dispose(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        # Act
        self.client.dispose()

        # Assert
        assert self.client.is_connected

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_subscribe_instrument(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        # Act
        self.client.subscribe_instrument(BTCUSDT)

        # Assert
        assert BTCUSDT in self.client.subscribed_instruments

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_subscribe_quote_ticks(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        # Act
        self.client.subscribe_quote_ticks(ETHUSDT)
        await asyncio.sleep(0.3)

        # Assert
        assert ETHUSDT in self.client.subscribed_quote_ticks
        assert self.data_engine.cache.has_quote_ticks(ETHUSDT)

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_subscribe_trade_ticks(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        # Act
        self.client.subscribe_trade_ticks(ETHUSDT)
        await asyncio.sleep(0.3)

        # Assert
        assert ETHUSDT in self.client.subscribed_trade_ticks
        assert self.data_engine.cache.has_trade_ticks(ETHUSDT)

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_subscribe_bars(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.5)  # Allow engine message queue to start

        bar_type = TestStubs.bartype_btcusdt_binance_100tick_last()

        # Act
        self.client.subscribe_bars(bar_type)

        # Assert
        assert bar_type in self.client.subscribed_bars

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_unsubscribe_instrument(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        self.client.subscribe_instrument(BTCUSDT)

        # Act
        self.client.unsubscribe_instrument(BTCUSDT)

        # Assert
        assert BTCUSDT not in self.client.subscribed_instruments

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_unsubscribe_quote_ticks(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        self.client.subscribe_quote_ticks(ETHUSDT)
        await asyncio.sleep(0.3)

        # Act
        self.client.unsubscribe_quote_ticks(ETHUSDT)

        # Assert
        assert ETHUSDT not in self.client.subscribed_quote_ticks

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_unsubscribe_trade_ticks(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        self.client.subscribe_trade_ticks(ETHUSDT)

        # Act
        self.client.unsubscribe_trade_ticks(ETHUSDT)

        # Assert
        assert ETHUSDT not in self.client.subscribed_trade_ticks

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_unsubscribe_bars(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        bar_type = TestStubs.bartype_btcusdt_binance_100tick_last()
        self.client.subscribe_bars(bar_type)

        # Act
        self.client.unsubscribe_bars(bar_type)

        # Assert
        assert bar_type not in self.client.subscribed_bars

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_request_instrument(self):
        # Arrange
        self.data_engine.start()
        await asyncio.sleep(0.5)  # Allow engine message queue to start

        # Act
        self.client.request_instrument(BTCUSDT, uuid4())
        await asyncio.sleep(0.5)

        # Assert
        # Instruments additionally requested on start
        assert self.data_engine.response_count == 1

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_request_instruments(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.5)  # Allow engine message queue to start

        # Act
        self.client.request_instruments(uuid4())
        await asyncio.sleep(0.5)

        # Assert
        # Instruments additionally requested on start
        assert self.data_engine.response_count == 1

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_request_quote_ticks(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        # Act
        self.client.request_quote_ticks(BTCUSDT, None, None, 0, uuid4())

        # Assert
        assert True  # Logs warning

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_request_trade_ticks(self):
        # Arrange
        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        handler = ObjectStorer()

        request = DataRequest(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                TradeTick,
                metadata={
                    "instrument_id": ETHUSDT,
                    "from_datetime": None,
                    "to_datetime": None,
                    "limit": 100,
                },
            ),
            callback=handler.store,
            request_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.send(request)

        await asyncio.sleep(1)

        # Assert
        assert self.data_engine.response_count == 1
        assert handler.count == 1

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_request_bars(self):
        # Arrange
        with open(TEST_PATH + "fetch_ohlcv.json") as response:
            fetch_ohlcv = json.load(response)

        self.mock_ccxt.fetch_ohlcv = fetch_ohlcv

        self.data_engine.start()  # Also starts client
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        handler = ObjectStorer()

        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)
        bar_type = BarType(instrument_id=ETHUSDT, bar_spec=bar_spec)

        request = DataRequest(
            client_id=ClientId(BINANCE.value),
            data_type=DataType(
                Bar,
                metadata={
                    "bar_type": bar_type,
                    "from_datetime": None,
                    "to_datetime": None,
                    "limit": 100,
                },
            ),
            callback=handler.store,
            request_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        # Act
        self.data_engine.send(request)

        await asyncio.sleep(0.3)

        # Assert
        assert self.data_engine.response_count == 1
        assert handler.count == 1
        assert len(handler.get_store()[0]) == 100

        # Tear Down
        self.data_engine.stop()
        await self.data_engine.get_run_queue_task()
