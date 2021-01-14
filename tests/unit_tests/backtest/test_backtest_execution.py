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

import unittest

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.cache import DataCache
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.commands import AmendOrder
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitBracketOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.providers import TestInstrumentProvider


BINANCE = Venue("BINANCE")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class BacktestExecClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.trader_id = TraderId("TESTER", "000")
        self.account_id = AccountId("BINANCE", "000")

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )
        self.portfolio.register_cache(DataCache(self.logger))

        self.analyzer = PerformanceAnalyzer()

        database = BypassExecutionDatabase(
            trader_id=self.trader_id,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            database=database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.exchange = SimulatedExchange(
            venue=Venue("BINANCE"),
            oms_type=OMSType.NETTING,
            generate_position_ids=True,
            is_frozen_account=False,
            starting_balances=[Money(1_000_000, USD)],
            instruments=[ETHUSDT_BINANCE],
            modules=[],
            exec_cache=self.exec_engine.cache,
            fill_model=FillModel(),
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            account_id=self.account_id,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER", "000"),
            clock=self.clock,
        )

    def test_is_connected_when_not_connected_returns_false(self):
        # Arrange

        # Act
        # Assert
        self.assertFalse(self.exec_client.is_connected)

    def test_connect(self):
        # Arrange
        # Act
        self.exec_client.connect()

        # Assert
        self.assertTrue(self.exec_client.is_connected)

    def test_disconnect(self):
        # Arrange
        self.exec_client.connect()

        # Act
        self.exec_client.disconnect()

        # Assert
        self.assertFalse(self.exec_client.is_connected)

    def test_reset(self):
        # Arrange
        # Act
        self.exec_client.reset()

        # Assert
        self.assertFalse(self.exec_client.is_connected)  # No exceptions raised

    def test_dispose(self):
        # Arrange
        # Act
        self.exec_client.dispose()

        # Assert
        self.assertFalse(self.exec_client.is_connected)  # No exceptions raised

    def test_submit_order_when_not_connected_logs_and_does_not_send(self):
        # Arrange
        strategy = TradingStrategy("000")
        order = self.order_factory.market(
            ETHUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity(100),
        )

        command = SubmitOrder(
            BINANCE,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        # Act
        self.exec_client.submit_order(command)

        # Assert
        self.assertEqual(OrderState.INITIALIZED, order.state)

    def test_submit_bracket_order_when_not_connected_logs_and_does_not_send(self):
        # Arrange
        strategy = TradingStrategy("000")
        entry = self.order_factory.market(
            ETHUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity(100),
        )

        bracket = self.order_factory.bracket(entry, Price("500.00000"))

        command = SubmitBracketOrder(
            BINANCE,
            self.trader_id,
            self.account_id,
            strategy.id,
            bracket,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        # Act
        self.exec_client.submit_bracket_order(command)

        # Assert
        self.assertEqual(OrderState.INITIALIZED, entry.state)

    def test_cancel_order_when_not_connected_logs_and_does_not_send(self):
        # Arrange
        order = self.order_factory.market(
            ETHUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity(100),
        )

        command = CancelOrder(
            BINANCE,
            self.trader_id,
            self.account_id,
            order.cl_ord_id,
            order.id,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        # Act
        self.exec_client.cancel_order(command)

        # Assert
        self.assertTrue(True)  # No exceptions raised

    def test_amend_order_when_not_connected_logs_and_does_not_send(self):
        # Arrange
        order = self.order_factory.stop_market(
            ETHUSDT_BINANCE.symbol,
            OrderSide.BUY,
            Quantity(100),
            Price("1000.00"),
        )

        command = AmendOrder(
            BINANCE,
            self.trader_id,
            self.account_id,
            order.cl_ord_id,
            Quantity(100),
            Price("1010.00"),
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        # Act
        self.exec_client.amend_order(command)

        # Assert
        self.assertTrue(True)  # No exceptions raised
