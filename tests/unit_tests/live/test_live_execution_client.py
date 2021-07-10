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
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClientFactory
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockLiveExecutionClient
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class TestLiveExecutionClientFactory:
    def test_create_when_not_implemented_raises_not_implemented_error(self):
        # Arrange
        self.loop = asyncio.new_event_loop()
        self.loop.set_debug(True)
        asyncio.set_event_loop(self.loop)

        self.clock = LiveClock()
        self.logger = LiveLogger(self.loop, self.clock)

        self.trader_id = TraderId("TESTER-000")

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        self.loop.set_debug(True)
        asyncio.set_event_loop(self.loop)

        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            trader_id=self.trader_id,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Act, Assert
        with pytest.raises(NotImplementedError):
            LiveExecutionClientFactory.create(
                name="IB",
                config={},
                engine=self.exec_engine,
                clock=self.clock,
                logger=self.logger,
            )


class TestLiveExecutionClient:
    def setup(self):
        # Fixture Setup

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        self.loop.set_debug(True)
        asyncio.set_event_loop(self.loop)

        self.clock = LiveClock()
        self.uuid_factory = UUIDFactory()
        self.logger = LiveLogger(self.loop, self.clock)

        self.trader_id = TraderId("TESTER-000")

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=self.clock,
        )

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        self.loop.set_debug(True)
        asyncio.set_event_loop(self.loop)

        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            portfolio=self.portfolio,
            trader_id=self.trader_id,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.risk_engine = LiveRiskEngine(
            loop=self.loop,
            exec_engine=self.exec_engine,
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.client = MockLiveExecutionClient(
            client_id=ClientId(SIM.value),
            venue_type=VenueType.ECN,
            account_id=TestStubs.account_id(),
            account_type=AccountType.CASH,
            base_currency=USD,
            engine=self.exec_engine,
            instrument_provider=InstrumentProvider(),
            clock=self.clock,
            logger=self.logger,
        )

        # Wire up components
        self.exec_engine.register_risk_engine(self.risk_engine)
        self.exec_engine.register_client(self.client)

        # Prepare components
        self.exec_engine.cache.add_instrument(AUDUSD_SIM)

    def teardown(self):
        self.client.dispose()

    def test_reconcile_state_given_no_order_and_not_in_cache_returns_false(self):
        async def run_test():
            # Arrange
            report = OrderStatusReport(
                client_order_id=ClientOrderId("O-123456"),
                venue_order_id=VenueOrderId("1"),
                order_state=OrderState.FILLED,
                filled_qty=Quantity.from_int(100000),
                timestamp_ns=0,
            )

            # Act
            result = await self.client.reconcile_state(
                report, order=None
            )  # <- order won't be in cache

            # Assert
            assert not result

        self.loop.run_until_complete(run_test())

    def test_reconcile_state_when_order_completed_returns_true_with_warning1(self):
        async def run_test():
            # Arrange
            self.exec_engine.start()
            self.risk_engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER-000"),
                self.clock,
                self.logger,
            )

            self.exec_engine.register_strategy(strategy)

            order = strategy.order_factory.stop_market(
                AUDUSD_SIM.id,
                OrderSide.BUY,
                Quantity.from_int(100000),
                Price.from_str("1.00000"),
            )

            submit_order = SubmitOrder(
                self.trader_id,
                strategy.id,
                PositionId.null(),
                order,
                self.uuid_factory.generate(),
                self.clock.timestamp_ns(),
            )

            self.risk_engine.execute(submit_order)
            await asyncio.sleep(0)  # Process queue
            self.exec_engine.process(TestStubs.event_order_submitted(order))
            await asyncio.sleep(0)  # Process queue
            self.exec_engine.process(TestStubs.event_order_accepted(order))
            await asyncio.sleep(0)  # Process queue
            self.exec_engine.process(TestStubs.event_order_canceled(order))
            await asyncio.sleep(0)  # Process queue

            report = OrderStatusReport(
                client_order_id=order.client_order_id,
                venue_order_id=VenueOrderId("1"),  # <-- from stub event
                order_state=OrderState.CANCELED,
                filled_qty=Quantity.zero(),
                timestamp_ns=0,
            )

            # Act
            result = await self.client.reconcile_state(report, order)

            # Assert
            assert result

        self.loop.run_until_complete(run_test())

    def test_reconcile_state_when_order_completed_returns_true_with_warning2(self):
        async def run_test():
            # Arrange
            self.exec_engine.start()
            self.risk_engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER-000"),
                self.clock,
                self.logger,
            )

            self.exec_engine.register_strategy(strategy)

            order = strategy.order_factory.limit(
                AUDUSD_SIM.id,
                OrderSide.BUY,
                Quantity.from_int(100000),
                Price.from_str("1.00000"),
            )

            submit_order = SubmitOrder(
                self.trader_id,
                strategy.id,
                PositionId.null(),
                order,
                self.uuid_factory.generate(),
                self.clock.timestamp_ns(),
            )

            self.risk_engine.execute(submit_order)
            await asyncio.sleep(0)  # Process queue
            self.exec_engine.process(TestStubs.event_order_submitted(order))
            await asyncio.sleep(0)  # Process queue
            self.exec_engine.process(TestStubs.event_order_accepted(order))
            await asyncio.sleep(0)  # Process queue
            self.exec_engine.process(TestStubs.event_order_filled(order, AUDUSD_SIM))
            await asyncio.sleep(0)  # Process queue

            report = OrderStatusReport(
                client_order_id=order.client_order_id,
                venue_order_id=VenueOrderId("1"),  # <-- from stub event
                order_state=OrderState.FILLED,
                filled_qty=Quantity.from_int(100000),
                timestamp_ns=0,
            )

            # Act
            result = await self.client.reconcile_state(report, order)

            # Assert
            assert result

        self.loop.run_until_complete(run_test())

    def test_reconcile_state_with_filled_order_when_trades_not_given_returns_false(
        self,
    ):
        async def run_test():
            # Arrange
            self.exec_engine.start()
            self.risk_engine.start()

            strategy = TradingStrategy(order_id_tag="001")
            strategy.register_trader(
                TraderId("TESTER-000"),
                self.clock,
                self.logger,
            )

            self.exec_engine.register_strategy(strategy)

            order = strategy.order_factory.limit(
                AUDUSD_SIM.id,
                OrderSide.BUY,
                Quantity.from_int(100000),
                Price.from_str("1.00000"),
            )

            submit_order = SubmitOrder(
                self.trader_id,
                strategy.id,
                PositionId.null(),
                order,
                self.uuid_factory.generate(),
                self.clock.timestamp_ns(),
            )

            self.risk_engine.execute(submit_order)
            await asyncio.sleep(0)  # Process queue
            self.exec_engine.process(TestStubs.event_order_submitted(order))
            await asyncio.sleep(0)  # Process queue
            self.exec_engine.process(TestStubs.event_order_accepted(order))
            await asyncio.sleep(0)  # Process queue

            report = OrderStatusReport(
                client_order_id=order.client_order_id,
                venue_order_id=VenueOrderId("1"),  # <-- from stub event
                order_state=OrderState.FILLED,
                filled_qty=Quantity.from_int(100000),
                timestamp_ns=0,
            )

            # Act
            result = await self.client.reconcile_state(report, order)

            # Assert
            assert not result

        self.loop.run_until_complete(run_test())
