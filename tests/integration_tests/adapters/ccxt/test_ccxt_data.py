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
import unittest
from unittest.mock import MagicMock

from nautilus_trader.adapters.ccxt.data import CCXTDataClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.messages import DataRequest
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.live.data import LiveDataEngine
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.tick import TradeTick
from nautilus_trader.trading.portfolio import Portfolio
from tests import PACKAGE_ROOT
from tests.test_kit.mocks import ObjectStorer
from tests.test_kit.stubs import TestStubs


TEST_PATH = PACKAGE_ROOT + "/integration_tests/adapters/ccxt/"

BINANCE = Venue("BINANCE")
BTCUSDT = Symbol("BTC/USDT", BINANCE)
ETHUSDT = Symbol("ETH/USDT", BINANCE)


# Monkey patch magic mock
# This allows the stubbing of calls to coroutines
MagicMock.__await__ = lambda x: async_magic().__await__()


# Dummy method for above
async def async_magic():
    return


class CCXTDataClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.trader_id = TraderId("TESTER", "001")

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        # Setup logging
        logger = LiveLogger(
            clock=self.clock,
            name=self.trader_id.value,
            level_console=LogLevel.INFO,
            level_file=LogLevel.DEBUG,
            level_store=LogLevel.WARNING,
        )

        self.logger = LiveLogger(self.clock)

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.mock_ccxt = MagicMock()
        self.mock_ccxt.name = "Binance"

        self.client = CCXTDataClient(
            client=self.mock_ccxt,
            engine=self.data_engine,
            clock=self.clock,
            logger=logger,
        )

        self.data_engine.register_client(self.client)

    def tearDown(self):
        self.loop.stop()
        self.loop.close()

    def test_connect(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            self.mock_ccxt.markets = instruments

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            # Act
            self.client.connect()

            # Assert
            self.assertTrue(self.client.is_connected())

            # Tear down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_disconnect(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            self.mock_ccxt.markets = instruments

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            # Act
            self.client.disconnect()
            await asyncio.sleep(0.3)

            # Assert
            self.assertFalse(self.client.is_connected())

            # Tear down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_reset_when_not_connected_successfully_resets(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            self.mock_ccxt.markets = instruments

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            self.data_engine.stop()
            await asyncio.sleep(0.3)  # Allow engine message queue to stop

            # Act
            self.client.reset()

            # Assert
            self.assertFalse(self.client.is_connected())

        self.loop.run_until_complete(run_test())

    def test_reset_when_connected_does_not_reset(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            self.mock_ccxt.markets = instruments

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            # Act
            self.client.reset()

            # Assert
            self.assertTrue(self.client.is_connected())

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_dispose_when_not_connected_does_not_dispose(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            self.mock_ccxt.markets = instruments

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            # Act
            self.client.dispose()

            # Assert
            self.assertTrue(self.client.is_connected())

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_subscribe_instrument(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            self.mock_ccxt.markets = instruments

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            # Act
            self.client.subscribe_instrument(BTCUSDT)

            # Assert
            self.assertIn(BTCUSDT, self.client.subscribed_instruments)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_subscribe_quote_ticks(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            with open(TEST_PATH + "res_order_book.json") as response:
                order_book = json.load(response)

            self.mock_ccxt.markets = instruments
            self.mock_ccxt.watch_order_book = order_book

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            # Act
            self.client.subscribe_quote_ticks(ETHUSDT)
            await asyncio.sleep(0.3)

            # Assert
            self.assertIn(ETHUSDT, self.client.subscribed_quote_ticks)
            self.assertTrue(self.data_engine.cache.has_quote_ticks(ETHUSDT))

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_subscribe_trade_ticks(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            with open(TEST_PATH + "res_trades.json") as response:
                trades = json.load(response)

            self.mock_ccxt.markets = instruments
            self.mock_ccxt.watch_trades = trades

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            # Act
            self.client.subscribe_trade_ticks(ETHUSDT)
            await asyncio.sleep(0.3)

            # Assert
            self.assertIn(ETHUSDT, self.client.subscribed_trade_ticks)
            self.assertTrue(self.data_engine.cache.has_trade_ticks(ETHUSDT))

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_subscribe_bars(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            self.mock_ccxt.markets = instruments

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            bar_type = TestStubs.bartype_btcusdt_binance_100tick_last()

            # Act
            self.client.subscribe_bars(bar_type)

            # Assert
            self.assertIn(bar_type, self.client.subscribed_bars)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_unsubscribe_instrument(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            self.mock_ccxt.markets = instruments

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            self.client.subscribe_instrument(BTCUSDT)

            # Act
            self.client.unsubscribe_instrument(BTCUSDT)

            # Assert
            self.assertNotIn(BTCUSDT, self.client.subscribed_instruments)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_unsubscribe_quote_ticks(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            with open(TEST_PATH + "res_order_book.json") as response:
                order_book = json.load(response)

            self.mock_ccxt.markets = instruments
            self.mock_ccxt.watch_order_book = order_book

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            self.client.subscribe_quote_ticks(ETHUSDT)
            await asyncio.sleep(0.3)

            # Act
            self.client.unsubscribe_quote_ticks(ETHUSDT)

            # Assert
            self.assertNotIn(ETHUSDT, self.client.subscribed_quote_ticks)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_unsubscribe_trade_ticks(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            with open(TEST_PATH + "res_trades.json") as response:
                trades = json.load(response)

            self.mock_ccxt.markets = instruments
            self.mock_ccxt.watch_trades = trades

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            self.client.subscribe_trade_ticks(ETHUSDT)

            # Act
            self.client.unsubscribe_trade_ticks(ETHUSDT)

            # Assert
            self.assertNotIn(ETHUSDT, self.client.subscribed_trade_ticks)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_unsubscribe_bars(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            with open(TEST_PATH + "res_order_book.json") as response:
                order_book = json.load(response)

            self.mock_ccxt.markets = instruments
            self.mock_ccxt.watch_order_book = order_book

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            bar_type = TestStubs.bartype_btcusdt_binance_100tick_last()
            self.client.subscribe_bars(bar_type)

            # Act
            self.client.unsubscribe_bars(bar_type)

            # Assert
            self.assertNotIn(bar_type, self.client.subscribed_bars)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_request_instrument(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            self.mock_ccxt.markets = instruments
            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            # Act
            self.client.request_instrument(BTCUSDT, uuid4())
            await asyncio.sleep(0.3)

            # Assert
            # Instruments additionally requested on start
            self.assertEqual(2, self.data_engine.response_count)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_request_instruments(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            self.mock_ccxt.markets = instruments
            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            # Act
            self.client.request_instruments(uuid4())
            await asyncio.sleep(0.3)

            # Assert
            # Instruments additionally requested on start
            self.assertEqual(2, self.data_engine.response_count)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_request_quote_ticks(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            self.mock_ccxt.markets = instruments
            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            # Act
            self.client.request_quote_ticks(BTCUSDT, None, None, 0, uuid4())

            # Assert
            self.assertTrue(True)  # Logs warning

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_request_trade_ticks(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            with open(TEST_PATH + "res_trades.json") as response:
                trades = json.load(response)

            self.mock_ccxt.markets = instruments
            self.mock_ccxt.fetch_trades = trades

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            handler = ObjectStorer()

            request = DataRequest(
                venue=BINANCE,
                data_type=TradeTick,
                metadata={
                    "Symbol": ETHUSDT,
                    "FromDateTime": None,
                    "ToDateTime": None,
                    "Limit": 100,
                },
                callback=handler.store,
                request_id=self.uuid_factory.generate(),
                request_timestamp=self.clock.utc_now(),
            )

            # Act
            self.data_engine.send(request)

            await asyncio.sleep(1)

            # Assert
            self.assertEqual(2, self.data_engine.response_count)
            self.assertEqual(1, handler.count)

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_request_bars(self):
        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            with open(TEST_PATH + "res_bars.json") as response:
                bars = json.load(response)

            self.mock_ccxt.markets = instruments
            self.mock_ccxt.fetch_ohlcv = bars

            self.data_engine.start()
            await asyncio.sleep(0.1)  # Allow engine message queue to start

            handler = ObjectStorer()

            bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)
            bar_type = BarType(symbol=ETHUSDT, bar_spec=bar_spec)

            request = DataRequest(
                venue=BINANCE,
                data_type=Bar,
                metadata={
                    "BarType": bar_type,
                    "FromDateTime": None,
                    "ToDateTime": None,
                    "Limit": 100,
                },
                callback=handler.store_2,
                request_id=self.uuid_factory.generate(),
                request_timestamp=self.clock.utc_now(),
            )

            # Act
            self.data_engine.send(request)

            await asyncio.sleep(0.3)

            # Assert
            self.assertEqual(2, self.data_engine.response_count)
            self.assertEqual(1, handler.count)
            self.assertEqual(100, len(handler.get_store()[0][1]))

            # Tear Down
            self.data_engine.stop()
            await self.data_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())
