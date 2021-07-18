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

from nautilus_trader.adapters.ccxt.execution import BinanceCCXTExecutionClient
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.message_bus import MessageBus
from nautilus_trader.trading.portfolio import Portfolio
from tests import TESTS_PACKAGE_ROOT
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


class TestBinanceExecutionClient:
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.trader_id = TestStubs.trader_id()
        self.account_id = AccountId(BINANCE.value, "001")

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

        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
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

        self.client = BinanceCCXTExecutionClient(
            client=self.mock_ccxt,
            account_id=self.account_id,
            account_type=AccountType.CASH,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.exec_engine.register_client(self.client)

    @pytest.mark.asyncio
    async def test_connect(self):
        # Arrange
        # Act
        self.exec_engine.start()  # Also connects clients
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        # Assert
        assert self.client.is_connected

        # Tear down
        self.exec_engine.stop()
        await self.exec_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_disconnect(self):
        # Arrange
        self.exec_engine.start()
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        # Act
        self.client.disconnect()
        await asyncio.sleep(0.3)

        # Assert
        assert not self.client.is_connected

        # Tear down
        self.exec_engine.stop()
        await self.exec_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_reset_when_not_connected_successfully_resets(self):
        # Arrange
        self.exec_engine.start()
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        self.exec_engine.stop()
        await asyncio.sleep(0.3)  # Allow engine message queue to stop

        # Act
        self.client.reset()

        # Assert
        assert not self.client.is_connected

    @pytest.mark.asyncio
    async def test_reset_when_connected_does_not_reset(self):
        # Arrange
        self.exec_engine.start()
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        # Act
        self.client.reset()

        # Assert
        assert self.client.is_connected

        # Tear Down
        self.exec_engine.stop()
        await self.exec_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_dispose_when_not_connected_does_not_dispose(self):
        # Arrange
        self.exec_engine.start()
        await asyncio.sleep(0.3)  # Allow engine message queue to start

        # Act
        self.client.dispose()

        # Assert
        assert self.client.is_connected

        # Tear Down
        self.exec_engine.stop()
        await self.exec_engine.get_run_queue_task()
