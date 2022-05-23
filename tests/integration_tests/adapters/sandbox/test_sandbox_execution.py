# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.sandbox.execution import SandboxExecutionClient
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from tests.integration_tests.adapters.betfair.test_kit import TestCommandStubs
from tests.test_kit.stubs.component import TestComponentStubs
from tests.test_kit.stubs.data import TestDataStubs
from tests.test_kit.stubs.execution import TestExecStubs
from tests.test_kit.stubs.identifiers import TestIdStubs


class TestSandboxExecutionClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)
        self.clock = LiveClock()
        self.venue = Venue("SANDBOX")
        self.trader_id = TestIdStubs.trader_id()
        self.account_id = AccountId(f"{self.venue.value}-001")

        # Setup logging
        self.logger = LiveLogger(loop=self.loop, clock=self.clock, level_stdout=LogLevel.DEBUG)
        self._log = LoggerAdapter("TestBetfairExecutionClient", self.logger)

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
            logger=self.logger,
        )
        self.instrument = TestInstrumentProvider.aapl_equity()

        self.cache = TestComponentStubs.cache()
        self.cache.add_instrument(self.instrument)
        self.cache.add_account(TestExecStubs.betting_account(account_id=self.account_id))

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        config = LiveExecEngineConfig()
        config.allow_cash_positions = True  # Retain original behaviour for now
        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=config,
        )

        self.client = SandboxExecutionClient(
            loop=asyncio.get_event_loop(),
            msgbus=self.msgbus,
            cache=self.cache,
            account_id=self.account_id,
            clock=self.clock,
            logger=self.logger,
            instrument_provider=InstrumentProvider(venue=self.venue, logger=self.logger),
            venue="SANDBOX",
            currency="USD",
            balance=100_000,
        )

        self.exec_engine.register_client(self.client)

        # Re-route exec engine messages through `handler`
        self.messages = []

        def handler(func):
            def inner(x):
                self.messages.append(x)
                return func(x)

            return inner

        def listener(x):
            print(x)

        self.msgbus.subscribe("*", listener)
        self.msgbus.deregister(endpoint="ExecEngine.execute", handler=self.exec_engine.execute)
        self.msgbus.register(
            endpoint="ExecEngine.execute", handler=handler(self.exec_engine.execute)
        )
        self.msgbus.deregister(endpoint="ExecEngine.process", handler=self.exec_engine.process)
        self.msgbus.register(
            endpoint="ExecEngine.process", handler=handler(self.exec_engine.process)
        )
        self.msgbus.deregister(
            endpoint="Portfolio.update_account", handler=self.portfolio.update_account
        )
        self.msgbus.register(
            endpoint="Portfolio.update_account", handler=handler(self.portfolio.update_account)
        )
        self.cache.add_quote_tick(
            TestDataStubs.quote_tick_3decimal(instrument_id=self.instrument.id)
        )

    @pytest.mark.asyncio
    async def test_submit_order_success(self):
        # Arrange
        command = TestCommandStubs.submit_order_command(
            order=TestExecStubs.limit_order(instrument_id=self.instrument.id)
        )

        # Act
        self.client.submit_order(command)
        await asyncio.sleep(0)

        # Assert
        submitted, accepted, filled = self.messages
        assert isinstance(submitted, OrderSubmitted)
        assert isinstance(accepted, OrderAccepted)
        assert isinstance(filled, OrderFilled)
        assert accepted.venue_order_id == VenueOrderId("1")

    # @pytest.mark.asyncio
    # async def test_submit_order_error(self):
    #     # Arrange
    #     command = TestCommandStubs.submit_order_command()
    #
    #     # Act
    #     self.client.submit_order(command)
    #     await asyncio.sleep(0)
    #
    #     # Assert
    #     submitted, rejected = self.messages
    #     assert isinstance(submitted, OrderSubmitted)
    #     assert isinstance(rejected, OrderRejected)
    #     assert rejected.reason == "PERMISSION_DENIED: ERROR_IN_ORDER"

    # @pytest.mark.asyncio
    # async def test_modify_order_success(self):
    #     # Arrange
    #     venue_order_id = VenueOrderId("240808576108")
    #     order = TestExecStubs.make_accepted_order(
    #         venue_order_id=venue_order_id, instrument_id=TestIdStubs.betting_instrument_id()
    #     )
    #     command = TestCommandStubs.modify_order_command(
    #         instrument_id=order.instrument_id,
    #         client_order_id=order.client_order_id,
    #         venue_order_id=venue_order_id,
    #     )
    #
    #     # Act
    #     self.cache.add_order(order, PositionId("1"))
    #     self.client.modify_order(command)
    #     await asyncio.sleep(0)
    #
    #     # Assert
    #     pending_update, updated = self.messages
    #     assert isinstance(pending_update, OrderPendingUpdate)
    #     assert isinstance(updated, OrderUpdated)
    #     assert updated.price == Price.from_str("0.02000")
    #
    # @pytest.mark.asyncio
    # async def test_modify_order_error_no_venue_id(self):
    #     # Arrange
    #     order = TestCommandStubs.make_submitted_order()
    #     self.cache.add_order(order, position_id=TestIdStubs.position_id())
    #
    #     command = TestCommandStubs.modify_order_command(
    #         instrument_id=order.instrument_id,
    #         client_order_id=order.client_order_id,
    #         venue_order_id="",
    #     )
    #
    #
    #     # Act
    #     self.client.modify_order(command)
    #     await asyncio.sleep(0)
    #
    #     # Assert
    #     pending_update, rejected = self.messages
    #     assert isinstance(pending_update, OrderPendingUpdate)
    #     assert isinstance(rejected, OrderModifyRejected)
    #     assert rejected.reason == "ORDER MISSING VENUE_ORDER_ID"
    #
    # @pytest.mark.asyncio
    # async def test_cancel_order_success(self):
    #     # Arrange
    #     order = TestCommandStubs.make_submitted_order()
    #     self.cache.add_order(order, position_id=TestIdStubs.position_id())
    #
    #     command = TestCommandStubs.cancel_order_command(
    #         instrument_id=order.instrument_id,
    #         client_order_id=order.client_order_id,
    #         venue_order_id=VenueOrderId("240564968665"),
    #     )
    #
    #     # Act
    #     self.client.cancel_order(command)
    #     await asyncio.sleep(0)
    #
    #     # Assert
    #     pending_cancel, cancelled = self.messages
    #     assert isinstance(pending_cancel, OrderPendingCancel)
    #     assert isinstance(cancelled, OrderCanceled)
    #
    # @pytest.mark.asyncio
    # async def test_cancel_order_fail(self):
    #     # Arrange
    #     order = TestCommandStubs.make_submitted_order()
    #     self.cache.add_order(order, position_id=TestIdStubs.position_id())
    #
    #     command = TestCommandStubs.cancel_order_command(
    #         instrument_id=order.instrument_id,
    #         client_order_id=order.client_order_id,
    #         venue_order_id=VenueOrderId("228302937743"),
    #     )
    #
    #     # Act
    #     self.client.cancel_order(command)
    #     await asyncio.sleep(0)
    #
    #     # Assert
    #     pending_cancel, cancelled = self.messages
    #     assert isinstance(pending_cancel, OrderPendingCancel)
    #     assert isinstance(cancelled, OrderCancelRejected)
