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

import pytest

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.message_bus import MessageBus
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockExecutionClient
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class TestLiveRiskEngine:
    def setup(self):
        # Fixture Setup
        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.trader_id = TestStubs.trader_id()
        self.account_id = TestStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=self.clock,
        )

        self.random_order_factory = OrderFactory(
            trader_id=TraderId("RANDOM-042"),
            strategy_id=StrategyId("S-042"),
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

        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            portfolio=self.portfolio,
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

        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            exec_engine=self.exec_engine,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config={},
        )

        self.exec_client = MockExecutionClient(
            client_id=ClientId("SIM"),
            venue_type=VenueType.ECN,
            account_id=TestStubs.account_id(),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.exec_engine.register_client(self.exec_client)

    def test_start_when_loop_not_running_logs(self):
        # Arrange
        # Act
        self.risk_engine.start()

        # Assert
        assert True  # No exceptions raised
        self.risk_engine.stop()

    def test_get_event_loop_returns_expected_loop(self):
        # Arrange
        # Act
        loop = self.risk_engine.get_event_loop()

        # Assert
        assert loop == self.loop

    def test_message_qsize_at_max_blocks_on_put_command(self):
        # Arrange
        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            exec_engine=self.exec_engine,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1},
        )

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            portfolio=self.portfolio,
            data_engine=self.data_engine,
            risk_engine=self.risk_engine,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)
        self.risk_engine.execute(submit_order)

        # Assert
        assert self.risk_engine.qsize() == 1
        assert self.risk_engine.command_count == 0

    def test_message_qsize_at_max_blocks_on_put_event(self):
        # Arrange
        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            exec_engine=self.exec_engine,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config={"qsize": 1},
        )

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            portfolio=self.portfolio,
            data_engine=self.data_engine,
            risk_engine=self.risk_engine,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        event = TestStubs.event_order_submitted(order)

        # Act
        self.risk_engine.execute(submit_order)
        self.risk_engine.process(event)  # Add over max size

        # Assert
        assert self.risk_engine.qsize() == 1
        assert self.risk_engine.event_count == 0

    @pytest.mark.asyncio
    async def test_start(self):
        # Arrange
        # Act
        self.risk_engine.start()
        await asyncio.sleep(0.1)

        # Assert
        assert self.risk_engine.state == ComponentState.RUNNING

        # Tear Down
        self.risk_engine.stop()

    @pytest.mark.asyncio
    async def test_kill_when_running_and_no_messages_on_queues(self):
        # Arrange
        # Act
        self.risk_engine.start()
        await asyncio.sleep(0)
        self.risk_engine.kill()

        # Assert
        assert self.risk_engine.state == ComponentState.STOPPED

    @pytest.mark.asyncio
    async def test_kill_when_not_running_with_messages_on_queue(self):
        # Arrange
        # Act
        self.risk_engine.kill()

        # Assert
        assert self.risk_engine.qsize() == 0

    @pytest.mark.asyncio
    async def test_execute_command_places_command_on_queue(self):
        # Arrange
        self.risk_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            portfolio=self.portfolio,
            data_engine=self.data_engine,
            risk_engine=self.risk_engine,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        submit_order = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.risk_engine.execute(submit_order)
        await asyncio.sleep(0.1)

        # Assert
        assert self.risk_engine.qsize() == 0
        assert self.risk_engine.command_count == 1

        # Tear Down
        self.risk_engine.stop()
        await self.risk_engine.get_run_queue_task()

    @pytest.mark.asyncio
    async def test_handle_position_opening_with_position_id_none(self):
        # Arrange
        self.risk_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            portfolio=self.portfolio,
            data_engine=self.data_engine,
            risk_engine=self.risk_engine,
            clock=self.clock,
            logger=self.logger,
        )

        order = strategy.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        event = TestStubs.event_order_submitted(order)

        # Act
        self.risk_engine.process(event)
        await asyncio.sleep(0.1)

        # Assert
        assert self.risk_engine.qsize() == 0
        assert self.risk_engine.event_count == 1

        # Tear Down
        self.risk_engine.stop()
        await self.risk_engine.get_run_queue_task()
