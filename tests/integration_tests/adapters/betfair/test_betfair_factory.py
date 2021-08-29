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
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.betfair.client.core import BetfairClient
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.factory import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factory import BetfairLiveExecutionClientFactory
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.msgbus.bus import MessageBus
from tests.test_kit.stubs import TestStubs


class TestBetfairFactory:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()

        self.trader_id = TestStubs.trader_id()
        self.venue = BETFAIR_VENUE
        self.account_id = AccountId(self.venue.value, "001")

        # Setup logging
        self.logger = LiveLogger(loop=self.loop, clock=self.clock, level_stdout=LogLevel.DEBUG)
        self._log = LoggerAdapter("TestBetfairExecutionClient", self.logger)

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )
        self.cache = TestStubs.cache()

    @pytest.mark.asyncio()
    def test_create(self):
        config = {
            "data_client": True,
            "exec_client": True,
            "base_currency": "AUD",
        }
        with patch.object(BetfairClient, "ssl_context", return_value=True):
            data_client = BetfairLiveDataClientFactory.create(
                loop=asyncio.get_event_loop(),
                name=BETFAIR_VENUE.value,
                config=config,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
            )
            exec_client = BetfairLiveExecutionClientFactory.create(
                loop=asyncio.get_event_loop(),
                name=BETFAIR_VENUE.value,
                config=config,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
            )

        # Assert
        assert BetfairDataClient == type(data_client)
        assert BetfairExecutionClient == type(exec_client)
        # TODO - assert login called
        # assert mock_login.assert_called_once_with()
