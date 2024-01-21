# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
from nautilus_trader.adapters.betfair.config import BetfairExecClientConfig
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.factories import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factories import BetfairLiveExecClientFactory
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestBetfairFactory:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()

        self.trader_id = TestIdStubs.trader_id()
        self.venue = BETFAIR_VENUE

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )
        self.cache = TestComponentStubs.cache()

    @pytest.mark.asyncio()
    def test_create(self):
        data_config = BetfairDataClientConfig(
            username="SOME_BETFAIR_USERNAME",
            password="SOME_BETFAIR_PASSWORD",
            app_key="SOME_BETFAIR_APP_KEY",
            cert_dir="SOME_BETFAIR_CERT_DIR",
            account_currency="GBP",
        )
        exec_config = BetfairExecClientConfig(
            username="SOME_BETFAIR_USERNAME",
            password="SOME_BETFAIR_PASSWORD",
            app_key="SOME_BETFAIR_APP_KEY",
            cert_dir="SOME_BETFAIR_CERT_DIR",
            account_currency="GBP",
        )

        data_client = BetfairLiveDataClientFactory.create(
            loop=asyncio.get_event_loop(),
            name=BETFAIR_VENUE.value,
            config=data_config,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        exec_client = BetfairLiveExecClientFactory.create(
            loop=asyncio.get_event_loop(),
            name=BETFAIR_VENUE.value,
            config=exec_config,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Assert
        assert BetfairDataClient is type(data_client)
        assert BetfairExecutionClient is type(exec_client)
