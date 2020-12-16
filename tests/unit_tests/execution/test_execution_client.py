# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import unittest

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.cache import DataCache
from nautilus_trader.execution.client import ExecutionClient
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import ModifyOrder
from nautilus_trader.model.commands import SubmitBracketOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


FXCM = Venue("FXCM")
USDJPY_FXCM = TestInstrumentProvider.default_fx_ccy(Symbol("USD/JPY", FXCM))
AUDUSD_FXCM = TestInstrumentProvider.default_fx_ccy(Symbol("AUD/USD", FXCM))


class ExecutionClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.trader_id = TraderId("TESTER", "000")
        self.account_id = TestStubs.account_id()

        portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )
        portfolio.register_cache(DataCache(self.logger))

        database = BypassExecutionDatabase(trader_id=self.trader_id, logger=self.logger)
        self.exec_engine = ExecutionEngine(
            database=database,
            portfolio=portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.venue = Venue("FXCM")

        self.client = ExecutionClient(
            venue=self.venue,
            account_id=self.account_id,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER", "000"),
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

    def test_connect_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.client.connect)

    def test_disconnect_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.client.disconnect)

    def test_reset_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.client.reset)

    def test_dispose_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.client.dispose)

    def test_is_connected_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.client.is_connected)

    def test_submit_order_raises_exception(self):
        order = self.order_factory.limit(
            AUDUSD_FXCM.symbol,
            OrderSide.SELL,
            Quantity(100000),
            Price("1.00000"),
        )

        command = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            StrategyId("SCALPER", "001"),
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.assertRaises(NotImplementedError, self.client.submit_order, command)

    def test_submit_bracket_order_raises_not_implemented_error(self):
        entry_order = self.order_factory.stop_market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("0.99995"),
        )

        # Act
        bracket_order = self.order_factory.bracket(
            entry_order,
            Price("0.99990"),
            Price("1.00010"),
        )

        command = SubmitBracketOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            StrategyId("SCALPER", "001"),
            bracket_order,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.assertRaises(NotImplementedError, self.client.submit_bracket_order, command)

    def test_modify_order_raises_not_implemented_error(self):
        # Arrange
        # Act
        command = ModifyOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            ClientOrderId("O-123456789"),
            Quantity(120000),
            Price("1.00000"),
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        # Assert
        self.assertRaises(NotImplementedError, self.client.modify_order, command)

    def test_cancel_order_raises_not_implemented_error(self):
        # Arrange
        # Act
        command = CancelOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            ClientOrderId("O-123456789"),
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        # Assert
        self.assertRaises(NotImplementedError, self.client.cancel_order, command)

    def test_handle_event_sends_to_execution_engine(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            AUDUSD_FXCM,
            PositionId("P-123456"),
            StrategyId("S", "001"),
            Price("1.00001"),
        )

        # Act
        self.client._handle_event(fill)  # Accessing protected method

        # Assert
        self.assertEqual(1, self.exec_engine.event_count)
