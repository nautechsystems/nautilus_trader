# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Optional

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.core.message import Event
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.live.data_client import LiveDataClient
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from nautilus_trader.trading.strategy import Strategy


class TestBaseClient:
    def setup(
        self,
        venue: Venue,
        instrument: Instrument,
        exec_client_factory: Optional[LiveExecClientFactory] = None,
        exec_client_config: Optional[LiveExecClientConfig] = None,
        data_client_factory: Optional[LiveDataClientFactory] = None,
        data_client_config: Optional[LiveDataClientConfig] = None,
        instrument_provider: Optional[InstrumentProvider] = None,
    ):
        self.exec_client_factory = exec_client_factory
        self.exec_client_config = exec_client_config
        self.data_client_factory = data_client_factory
        self.data_client_config = data_client_config
        self.instrument_provider = instrument_provider

        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.venue = venue
        self.instrument = instrument
        self.instrument_provider = instrument_provider

        # Identifiers
        self.instrument_id = self.instrument.id
        self.trader_id = TestIdStubs.trader_id()
        self.account_id = AccountId(f"{self.venue.value}-001")
        self.venue_order_id = VenueOrderId("V-1")
        self.client_order_id = ClientOrderId("C-1")

        # Components
        self.clock = LiveClock()
        self.logger: Logger = Logger(self.loop, self.clock)
        self.msgbus = MessageBus(
            self.trader_id,
            self.clock,
            self.logger,
        )
        self.cache = TestComponentStubs.cache()
        self.portfolio = Portfolio(
            self.msgbus,
            self.cache,
            self.clock,
            self.logger,
        )
        self.data_engine = DataEngine(
            self.msgbus,
            self.cache,
            self.clock,
            self.logger,
        )
        self.exec_engine = ExecutionEngine(
            self.msgbus,
            self.cache,
            self.clock,
            self.logger,
        )
        self.risk_engine = RiskEngine(
            self.portfolio,
            self.msgbus,
            self.cache,
            self.clock,
            self.logger,
        )

        # Create clients & strategy
        self._exec_client: Optional[LiveExecutionClient] = None
        self._data_client: Optional[LiveDataClient] = None
        self.strategy = Strategy()
        self.strategy.register(
            self.trader_id,
            self.portfolio,
            self.msgbus,
            self.cache,
            self.clock,
            self.logger,
        )

        # Capture events flowing through engines
        self.order_events: list[Event] = []
        self.msgbus.subscribe("events.order*", self.order_events.append)

        self.logs: list[str] = []
        self.logger.register_sink(self.logs.append)

    @property
    def data_client(self) -> LiveDataClient:
        if self._data_client is None:
            self._data_client = self.data_client_factory.create(
                loop=self.loop,
                name=self.venue.value,
                config=self.data_client_config,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
            )
            self.data_engine.register_client(self._data_client)
        return self._data_client

    @property
    def exec_client(self) -> LiveExecutionClient:
        if self._exec_client is None:
            self._exec_client = self.exec_client_factory.create(
                loop=self.loop,
                name=self.venue.value,
                config=self.exec_client_config,
                msgbus=self.msgbus,
                cache=self.cache,
                clock=self.clock,
                logger=self.logger,
            )
            self.exec_engine.register_client(self._exec_client)
        return self._exec_client
