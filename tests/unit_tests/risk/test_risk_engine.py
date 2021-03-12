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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.data.cache import DataCache
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.commands import SubmitBracketOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import MockExecutionClient
from tests.test_kit.mocks import MockExecutionDatabase
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs

AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestRiskEngine:

    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = TestLogger(self.clock)

        self.trader_id = TraderId("TESTER", "000")
        self.account_id = TestStubs.account_id()
        self.venue = Venue("SIM")

        self.portfolio = Portfolio(
            clock=self.clock,
            logger=self.logger,
        )
        self.portfolio.register_cache(DataCache(self.logger))

        self.database = MockExecutionDatabase(trader_id=self.trader_id, logger=self.logger)
        self.exec_engine = ExecutionEngine(
            database=self.database,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_client = MockExecutionClient(
            self.venue,
            self.account_id,
            self.exec_engine,
            self.clock,
            self.logger,
        )

        self.risk_engine = RiskEngine(
            exec_engine=self.exec_engine,
            portfolio=self.portfolio,
            clock=self.clock,
            logger=self.logger,
            config={},
        )

        self.exec_engine.register_client(self.exec_client)
        self.exec_engine.register_risk_engine(self.risk_engine)

    def test_set_block_all_orders_changes_flag_value(self):
        # Arrange
        # Act
        self.risk_engine.set_block_all_orders()

        # Assert
        assert self.risk_engine.block_all_orders

    def test_approve_order_when_engine_not_overridden_then_approves(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.security,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        # Act
        self.risk_engine.approve_order(submit_order)

        # Assert
        assert submit_order.approved

    def test_approve_bracket_when_engine_not_overridden_then_approves(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        entry = strategy.order_factory.market(
            AUDUSD_SIM.security,
            OrderSide.BUY,
            Quantity(100000),
        )

        bracket = strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price("1.00000"),
            take_profit=Price("1.00010"),
        )

        submit_bracket = SubmitBracketOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy.id,
            bracket,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        # Act
        self.risk_engine.approve_bracket(submit_bracket)

        # Assert
        assert submit_bracket.approved

    def test_submit_order_when_block_all_orders_true_then_denies_order(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        order = strategy.order_factory.market(
            AUDUSD_SIM.security,
            OrderSide.BUY,
            Quantity(100000),
        )

        submit_order = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy.id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.risk_engine.set_block_all_orders()

        # Act
        self.exec_engine.execute(submit_order)

        # Assert
        assert not submit_order.approved

    def test_submit_bracket_when_block_all_orders_true_then_denies_order(self):
        # Arrange
        self.exec_engine.start()

        strategy = TradingStrategy(order_id_tag="001")
        strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.logger,
        )

        self.exec_engine.register_strategy(strategy)

        entry = strategy.order_factory.market(
            AUDUSD_SIM.security,
            OrderSide.BUY,
            Quantity(100000),
        )

        bracket = strategy.order_factory.bracket(
            entry_order=entry,
            stop_loss=Price("1.00000"),
            take_profit=Price("1.00010"),
        )

        submit_bracket = SubmitBracketOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            strategy.id,
            bracket,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.risk_engine.set_block_all_orders()

        # Act
        self.risk_engine.approve_bracket(submit_bracket)

        # Assert
        assert not submit_bracket.approved
