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

from nautilus_trader.adapters.ftx.factories import FTXLiveDataClientFactory
from nautilus_trader.adapters.ftx.factories import FTXLiveExecutionClientFactory
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.msgbus.bus import MessageBus
from tests.test_kit.mocks import MockCacheDatabase
from tests.test_kit.stubs import TestStubs


class TestFTXFactories:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.clock = LiveClock()
        self.logger = LiveLogger(
            loop=self.loop,
            clock=self.clock,
            level_stdout=LogLevel.DEBUG,
        )

        self.trader_id = TestStubs.trader_id()
        self.strategy_id = TestStubs.strategy_id()
        self.account_id = TestStubs.account_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )

        self.cache_db = MockCacheDatabase(
            logger=self.logger,
        )

        self.cache = Cache(
            database=self.cache_db,
            logger=self.logger,
        )

    def test_ftx_live_data_client_factory(self, ftx_http_client):
        # Arrange, Act
        data_client = FTXLiveDataClientFactory.create(
            loop=self.loop,
            name="FTX",
            config={"api_key": "SOME_FTX_API_KEY", "api_secret": "SOME_FTX_API_SECRET"},
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        assert data_client is not None

    def test_ftx_live_exec_client_factory(self, ftx_http_client):
        # Arrange, Act
        exec_client = FTXLiveExecutionClientFactory.create(
            loop=self.loop,
            name="FTX",
            config={"api_key": "SOME_FTX_API_KEY", "api_secret": "SOME_FTX_API_SECRET"},
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        assert exec_client is not None
