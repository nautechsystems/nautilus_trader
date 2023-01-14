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

import pytest

from nautilus_trader.adapters.sandbox.execution import SandboxExecutionClient
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.order import OrderCancelRejected
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderModifyRejected
from nautilus_trader.model.events.order import OrderPendingCancel
from nautilus_trader.model.events.order import OrderPendingUpdate
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestSandboxExecutionClient:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)
        self.clock = LiveClock()
        self.venue = Venue("NASDAQ")
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
        self.cache.add_account(TestExecStubs.cash_account(account_id=self.account_id))
        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        config = LiveExecEngineConfig()
        self.exec_engine = LiveExecutionEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            config=config,
        )

        self.data_engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        SandboxExecutionClient.INSTRUMENTS = [self.instrument]
        self.client = SandboxExecutionClient(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
            venue=self.venue.value,
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
            endpoint="ExecEngine.execute",
            handler=handler(self.exec_engine.execute),
        )
        self.msgbus.deregister(endpoint="ExecEngine.process", handler=self.exec_engine.process)
        self.msgbus.register(
            endpoint="ExecEngine.process",
            handler=handler(self.exec_engine.process),
        )
        self.msgbus.deregister(
            endpoint="Portfolio.update_account",
            handler=self.portfolio.update_account,
        )
        self.msgbus.register(
            endpoint="Portfolio.update_account",
            handler=handler(self.portfolio.update_account),
        )
        self.cache.add_quote_tick(
            TestDataStubs.quote_tick_3decimal(instrument_id=self.instrument.id),
        )

    def _make_quote_tick(self):
        return QuoteTick(
            instrument_id=self.instrument.id,
            bid=Price.from_int(10),
            ask=Price.from_int(10),
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_init=self.clock.timestamp_ns(),
            ts_event=0,
        )

    @pytest.mark.asyncio
    async def test_connect(self):
        self.client.connect()
        assert isinstance(self.client.exchange, SimulatedExchange)

    @pytest.mark.asyncio
    async def test_submit_order_success(self):
        # Arrange
        self.client.connect()
        command = TestCommandStubs.submit_order_command(
            order=TestExecStubs.limit_order(instrument_id=self.instrument.id),
        )

        # Act
        self.client.submit_order(command)
        self.client.on_data(self._make_quote_tick())

        # Assert
        submitted, accepted, filled = self.messages
        assert isinstance(submitted, OrderSubmitted)
        assert isinstance(accepted, OrderAccepted)
        assert isinstance(filled, OrderFilled)
        assert accepted.venue_order_id == VenueOrderId("NASDAQ-1-001")

    @pytest.mark.asyncio
    async def test_modify_order_success(self):
        # Arrange
        self.client.connect()
        submit_order_command = TestCommandStubs.submit_order_command(
            order=TestExecStubs.limit_order(
                instrument_id=self.instrument.id,
                price=Price.from_str("0.01"),
            ),
        )
        self.client.submit_order(submit_order_command)
        self.client.on_data(self._make_quote_tick())
        order_accepted = self.messages[1]

        # Act

        command = TestCommandStubs.modify_order_command(
            order=order_accepted,
            price=Price.from_str("0.01"),
            quantity=Quantity.from_int(200),
        )
        self.client.modify_order(command)
        self.client.on_data(self._make_quote_tick())

        # Assert
        print(self.messages)
        _, _, pending_update, updated = self.messages
        assert isinstance(pending_update, OrderPendingUpdate)
        assert isinstance(updated, OrderUpdated)
        assert updated.price == Price.from_str("0.01")

    @pytest.mark.asyncio
    async def test_modify_order_error_no_venue_id(self):
        # Arrange
        self.client.connect()
        submit_order_command = TestCommandStubs.submit_order_command(
            order=TestExecStubs.limit_order(
                instrument_id=self.instrument.id,
                price=Price.from_str("0.01"),
            ),
        )
        self.client.submit_order(submit_order_command)
        self.client.on_data(self._make_quote_tick())
        order_accepted = self.messages[1]

        # Act
        command = TestCommandStubs.modify_order_command(
            instrument_id=order_accepted.instrument_id,
            client_order_id=ClientOrderId("NOT-AN-ID"),
            price=Price.from_str("0.01"),
            quantity=Quantity.from_int(200),
        )
        self.client.modify_order(command)
        self.client.on_data(self._make_quote_tick())

        # Assert
        _, _, rejected = self.messages
        assert isinstance(rejected, OrderModifyRejected)
        assert rejected.reason == "ClientOrderId('NOT-AN-ID') not found"

    @pytest.mark.asyncio
    async def test_cancel_order_success(self):
        # Arrange
        self.client.connect()
        submit_order_command = TestCommandStubs.submit_order_command(
            order=TestExecStubs.limit_order(
                instrument_id=self.instrument.id,
                price=Price.from_str("0.01"),
            ),
        )
        self.client.submit_order(submit_order_command)
        self.client.on_data(self._make_quote_tick())
        order_accepted = self.messages[1]
        submit_order_command.order.apply(self.messages[0])
        submit_order_command.order.apply(self.messages[1])
        self.cache.add_order(submit_order_command.order, PositionId("1"))

        # Act
        command = TestCommandStubs.cancel_order_command(
            instrument_id=order_accepted.instrument_id,
            client_order_id=order_accepted.client_order_id,
            venue_order_id=order_accepted.venue_order_id,
        )
        self.client.cancel_order(command)
        self.client.on_data(self._make_quote_tick())

        # Assert
        _, _, pending_cancel, cancelled = self.messages
        assert isinstance(pending_cancel, OrderPendingCancel)
        assert isinstance(cancelled, OrderCanceled)

    @pytest.mark.asyncio
    async def test_cancel_order_fail(self):
        # Arrange
        self.client.connect()
        submit_order_command = TestCommandStubs.submit_order_command(
            order=TestExecStubs.limit_order(
                instrument_id=self.instrument.id,
                price=Price.from_str("0.01"),
            ),
        )
        self.client.submit_order(submit_order_command)
        self.client.on_data(self._make_quote_tick())
        order_accepted = self.messages[1]
        submit_order_command.order.apply(self.messages[0])
        submit_order_command.order.apply(self.messages[1])
        self.cache.add_order(submit_order_command.order, PositionId("1"))

        # Act
        command = TestCommandStubs.cancel_order_command(
            instrument_id=order_accepted.instrument_id,
            client_order_id=ClientOrderId("111"),
        )
        self.client.cancel_order(command)
        self.client.on_data(self._make_quote_tick())

        # Assert
        _, _, rejected = self.messages
        assert isinstance(rejected, OrderCancelRejected)
