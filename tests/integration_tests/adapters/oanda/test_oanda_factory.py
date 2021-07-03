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

from nautilus_trader.adapters.oanda.data import OandaDataClient
from nautilus_trader.adapters.oanda.factories import OandaDataClientFactory
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.stubs import TestStubs


class TestOandaDataClientFactory:
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.trader_id = TraderId("TESTER-001")

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        self.loop.set_debug(True)
        asyncio.set_event_loop(self.loop)

        self.logger = LiveLogger(
            loop=self.loop,
            clock=self.clock,
        )

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
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

    def test_create(self):
        # Arrange
        config = {
            "api_token": "OANDA_API_TOKEN",  # value is the environment variable name
            "account_id": "OANDA_ACCOUNT_ID",  # value is the environment variable name
        }

        # Act
        client = OandaDataClientFactory.create(
            name="OANDA",
            config=config,
            engine=self.data_engine,
            clock=self.clock,
            logger=self.logger,
        )

        # Assert
        assert type(client) == OandaDataClient
