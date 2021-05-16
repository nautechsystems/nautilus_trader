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
from unittest.mock import MagicMock

from nautilus_trader.adapters.ccxt.data import CCXTDataClient
from nautilus_trader.adapters.ccxt.execution import BinanceCCXTExecutionClient
from nautilus_trader.adapters.ccxt.factories import CCXTDataClientFactory
from nautilus_trader.adapters.ccxt.factories import CCXTExecutionClientFactory
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.execution.database import InMemoryExecutionDatabase
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.portfolio import Portfolio


class TestCCXTDataClientFactory:
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.trader_id = TraderId("TESTER", "001")

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        self.logger = LiveLogger(
            loop=self.loop,
            clock=self.clock,
        )

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

        database = InMemoryExecutionDatabase(
            trader_id=self.trader_id, logger=self.logger
        )
        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            database=database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

    def test_create(self):
        # Arrange
        config = {
            "account_id": "BINANCE_ACCOUNT_ID",  # value is the environment variable name
            "api_key": "BINANCE_API_KEY",  # value is the environment variable name
            "api_secret": "BINANCE_API_SECRET",  # value is the environment variable name
        }

        # Mock client
        mock_bitmex = MagicMock()
        mock_bitmex.name = "binance"

        # Mock constructor method to return the mock client
        client_cls = MagicMock()
        client_cls.return_value = mock_bitmex

        # Act
        data_client = CCXTDataClientFactory.create(
            name="CCXT-BINANCE",
            config=config,
            engine=self.data_engine,
            clock=self.clock,
            logger=self.logger,
            client_cls=client_cls,
        )

        # Assert
        assert type(data_client) == CCXTDataClient
        assert data_client.id == ClientId("BINANCE")


class TestCCXTExecClientFactory:
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.trader_id = TraderId("TESTER", "001")

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        self.logger = LiveLogger(
            loop=self.loop,
            clock=self.clock,
        )

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

        database = InMemoryExecutionDatabase(
            trader_id=self.trader_id, logger=self.logger
        )
        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            database=database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

    def test_create(self):
        # Arrange
        config = {
            "account_id": "BINANCE_ACCOUNT_ID",  # value is the environment variable name
            "api_key": "BINANCE_API_KEY",  # value is the environment variable name
            "api_secret": "BINANCE_API_SECRET",  # value is the environment variable name
        }

        # Mock client
        mock_bitmex = MagicMock()
        mock_bitmex.name = "binance"

        # Mock constructor method to return the mock client
        client_cls = MagicMock()
        client_cls.return_value = mock_bitmex

        # Act
        client = CCXTExecutionClientFactory.create(
            name="CCXT-BINANCE",
            config=config,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
            client_cls=client_cls,
        )

        # Assert
        assert type(client) == BinanceCCXTExecutionClient
        assert client.id == ClientId("BINANCE")
