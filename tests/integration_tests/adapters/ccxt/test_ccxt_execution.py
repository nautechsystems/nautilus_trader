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

from nautilus_trader.adapters.ccxt.execution import CCXTExecutionClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.trading.portfolio import Portfolio
from tests import TESTS_PACKAGE_ROOT


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


class CCXTExecutionClientTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.trader_id = TraderId("TESTER", "001")
        self.account_id = AccountId("BINANCE", "001")

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
            run_in_process=False,
        )

        self.logger = LiveLogger(self.clock)

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )

        database = BypassExecutionDatabase(trader_id=self.trader_id, logger=self.logger)
        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            database=database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        with open(TEST_PATH + "markets.json") as response:
            markets = json.load(response)

        with open(TEST_PATH + "currencies.json") as response:
            currencies = json.load(response)

        with open(TEST_PATH + "fetch_balance.json") as response:
            fetch_balance = json.load(response)

        with open(TEST_PATH + "watch_balance.json") as response:
            watch_balance = json.load(response)

        self.mock_ccxt = MagicMock()
        self.mock_ccxt.name = "Binance"
        self.mock_ccxt.precisionMode = 2
        self.mock_ccxt.has = {
            "fetchBalance": True,
            "watchBalance": True,
            "watchMyTrades": True,
        }
        self.mock_ccxt.markets = markets
        self.mock_ccxt.currencies = currencies
        self.mock_ccxt.fetch_balance = fetch_balance
        self.mock_ccxt.watch_balance = watch_balance

        self.client = CCXTExecutionClient(
            client=self.mock_ccxt,
            account_id=self.account_id,
            engine=self.exec_engine,
            clock=self.clock,
            logger=logger,
        )

        self.exec_engine.register_client(self.client)

    def tearDown(self):
        self.loop.stop()
        self.loop.close()

    def test_connect(self):
        async def run_test():
            # Arrange
            # Act
            self.exec_engine.start()  # Also connects clients
            await asyncio.sleep(0.3)  # Allow engine message queue to start

            # Assert
            self.assertTrue(self.client.is_connected)

            # Tear down
            self.exec_engine.stop()
            await self.exec_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_disconnect(self):
        async def run_test():
            # Arrange
            self.exec_engine.start()
            await asyncio.sleep(0.3)  # Allow engine message queue to start

            # Act
            self.client.disconnect()
            await asyncio.sleep(0.3)

            # Assert
            self.assertFalse(self.client.is_connected)

            # Tear down
            self.exec_engine.stop()
            await self.exec_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_reset_when_not_connected_successfully_resets(self):
        async def run_test():
            # Arrange
            self.exec_engine.start()
            await asyncio.sleep(0.3)  # Allow engine message queue to start

            self.exec_engine.stop()
            await asyncio.sleep(0.3)  # Allow engine message queue to stop

            # Act
            self.client.reset()

            # Assert
            self.assertFalse(self.client.is_connected)

        self.loop.run_until_complete(run_test())

    def test_reset_when_connected_does_not_reset(self):
        async def run_test():
            # Arrange
            self.exec_engine.start()
            await asyncio.sleep(0.3)  # Allow engine message queue to start

            # Act
            self.client.reset()

            # Assert
            self.assertTrue(self.client.is_connected)

            # Tear Down
            self.exec_engine.stop()
            await self.exec_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())

    def test_dispose_when_not_connected_does_not_dispose(self):
        async def run_test():
            # Arrange
            self.exec_engine.start()
            await asyncio.sleep(0.3)  # Allow engine message queue to start

            # Act
            self.client.dispose()

            # Assert
            self.assertTrue(self.client.is_connected)

            # Tear Down
            self.exec_engine.stop()
            await self.exec_engine.get_run_queue_task()

        self.loop.run_until_complete(run_test())
