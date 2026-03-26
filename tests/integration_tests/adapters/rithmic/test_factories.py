# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic.config import RithmicEnvironment
from nautilus_trader.adapters.rithmic.config import RithmicExecClientConfig
from nautilus_trader.adapters.rithmic.data import RITHMIC_VENUE
from nautilus_trader.adapters.rithmic.data import RithmicLiveDataClient
from nautilus_trader.adapters.rithmic.execution import RithmicLiveExecutionClient
from nautilus_trader.adapters.rithmic.factories import RithmicLiveDataClientFactory
from nautilus_trader.adapters.rithmic.factories import RithmicLiveExecClientFactory
from nautilus_trader.adapters.rithmic.providers import RithmicInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.test_kit.mocks.cache_database import MockCacheDatabase
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestRithmicFactories:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        self.loop = request.getfixturevalue("event_loop")
        self.clock = LiveClock()
        self.msgbus = MessageBus(
            trader_id=TestIdStubs.trader_id(),
            clock=self.clock,
        )
        self.cache = Cache(database=MockCacheDatabase())

    def test_create_rithmic_live_data_client(self):
        config = RithmicDataClientConfig(
            environment=RithmicEnvironment.DEMO,
            username="u",
            password="p",
            system_name="Apex",
        )

        data_client = RithmicLiveDataClientFactory.create(
            loop=self.loop,
            name="RITHMIC",
            config=config,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        assert isinstance(data_client, RithmicLiveDataClient)
        assert data_client.venue == RITHMIC_VENUE
        assert data_client._config == config
        assert isinstance(data_client._instrument_provider, RithmicInstrumentProvider)

    def test_create_rithmic_live_exec_client(self):
        config = RithmicExecClientConfig(
            environment=RithmicEnvironment.DEMO,
            username="u",
            password="p",
            system_name="Apex",
            account_id="A1",
        )

        exec_client = RithmicLiveExecClientFactory.create(
            loop=self.loop,
            name="RITHMIC",
            config=config,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        assert isinstance(exec_client, RithmicLiveExecutionClient)
        assert exec_client.venue == RITHMIC_VENUE
        assert exec_client._config == config
        assert exec_client.account_id == AccountId("RITHMIC-A1")
