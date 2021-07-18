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

from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.commands.trading import CancelOrder
from nautilus_trader.model.commands.trading import SubmitBracketOrder
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.commands.trading import UpdateOrder
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.message_bus import MessageBus
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


BINANCE = Venue("BINANCE")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestBacktestExecClientTests:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.trader_id = TestStubs.trader_id()
        self.account_id = AccountId("BINANCE", "000")

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

        self.exec_engine = ExecutionEngine(
            trader_id=self.trader_id,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exchange = SimulatedExchange(
            venue=Venue("BINANCE"),
            venue_type=VenueType.EXCHANGE,
            oms_type=OMSType.NETTING,
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency account
            starting_balances=[Money(1_000_000, USDT)],
            is_frozen_account=False,
            instruments=[ETHUSDT_BINANCE],
            modules=[],
            cache=self.exec_engine.cache,
            fill_model=FillModel(),
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            account_id=self.account_id,
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency account
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            clock=self.clock,
        )

    def test_is_connected_when_not_connected_returns_false(self):
        # Arrange

        # Act
        # Assert
        assert not self.exec_client.is_connected

    def test_connect(self):
        # Arrange
        # Act
        self.exec_client.connect()

        # Assert
        assert self.exec_client.is_connected

    def test_disconnect(self):
        # Arrange
        self.exec_client.connect()

        # Act
        self.exec_client.disconnect()

        # Assert
        assert not self.exec_client.is_connected

    def test_reset(self):
        # Arrange
        # Act
        self.exec_client.reset()

        # Assert
        assert not self.exec_client.is_connected

    def test_dispose(self):
        # Arrange
        # Act
        self.exec_client.dispose()

        # Assert
        assert not self.exec_client.is_connected

    def test_submit_order_when_not_connected_logs_and_does_not_send(self):
        # Arrange
        strategy = TradingStrategy("000")
        order = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(100),
        )

        command = SubmitOrder(
            self.trader_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_client.submit_order(command)

        # Assert
        assert order.state == OrderState.INITIALIZED

    def test_submit_bracket_order_when_not_connected_logs_and_does_not_send(self):
        # Arrange
        strategy = TradingStrategy("000")
        entry = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(100),
        )

        bracket = self.order_factory.bracket(
            entry,
            Price.from_str("500.00000"),
            Price.from_str("600.00000"),
        )

        command = SubmitBracketOrder(
            self.trader_id,
            strategy.id,
            bracket,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_client.submit_bracket_order(command)

        # Assert
        assert entry.state == OrderState.INITIALIZED

    def test_cancel_order_when_not_connected_logs_and_does_not_send(self):
        # Arrange
        order = self.order_factory.market(
            ETHUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(100),
        )

        command = CancelOrder(
            self.trader_id,
            self.order_factory.strategy_id,
            order.instrument_id,
            order.client_order_id,
            order.venue_order_id,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_client.cancel_order(command)

        # Assert
        assert True  # No exceptions raised

    def test_update_order_when_not_connected_logs_and_does_not_send(self):
        # Arrange
        order = self.order_factory.stop_market(
            ETHUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(100),
            Price.from_str("1000.00"),
        )

        command = UpdateOrder(
            self.trader_id,
            order.strategy_id,
            order.instrument_id,
            order.client_order_id,
            order.venue_order_id,
            Quantity.from_int(100),
            Price.from_str("1010.00"),
            None,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Act
        self.exec_client.update_order(command)

        # Assert
        assert True  # No exceptions raised
