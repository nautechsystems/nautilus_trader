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

from nautilus_trader.backtest.data_client import BacktestMarketDataClient
from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.logging import Logger
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.trading.portfolio import Portfolio
from nautilus_trader.trading.strategy import TradingStrategy
from nautilus_trader.trading.trader import Trader
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TraderTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        clock = TestClock()
        logger = Logger(clock)

        trader_id = TraderId("TESTER-000")
        account_id = TestStubs.account_id()

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            cache=self.cache,
            clock=clock,
            logger=logger,
        )

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            cache=self.cache,
            clock=clock,
            logger=logger,
            config={"use_previous_close": False},
        )

        self.data_engine.process(USDJPY_SIM)

        self.exec_engine = ExecutionEngine(
            portfolio=self.portfolio,
            cache=self.cache,
            clock=clock,
            logger=logger,
        )

        self.exchange = SimulatedExchange(
            venue=Venue("SIM"),
            venue_type=VenueType.ECN,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            is_frozen_account=False,
            cache=self.exec_engine.cache,
            instruments=[USDJPY_SIM],
            modules=[],
            fill_model=FillModel(),
            clock=clock,
            logger=logger,
        )

        self.data_client = BacktestMarketDataClient(
            client_id=ClientId("SIM"),
            engine=self.data_engine,
            clock=clock,
            logger=logger,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            account_id=account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            engine=self.exec_engine,
            clock=clock,
            logger=logger,
        )

        self.risk_engine = RiskEngine(
            exec_engine=self.exec_engine,
            portfolio=self.portfolio,
            cache=self.cache,
            clock=clock,
            logger=logger,
        )

        # Wire up components
        self.data_engine.register_client(self.data_client)
        self.exec_engine.register_risk_engine(self.risk_engine)
        self.exec_engine.register_client(self.exec_client)

        strategies = [
            TradingStrategy("001"),
            TradingStrategy("002"),
        ]

        self.trader = Trader(
            trader_id=trader_id,
            strategies=strategies,
            portfolio=self.portfolio,
            data_engine=self.data_engine,
            risk_engine=self.risk_engine,
            exec_engine=self.exec_engine,
            clock=clock,
            logger=logger,
        )

    def test_initialize_trader(self):
        # Arrange
        # Act
        trader_id = self.trader.id

        # Assert
        self.assertEqual(TraderId("TESTER-000"), trader_id)
        self.assertEqual(ComponentState.INITIALIZED, self.trader.state)
        self.assertEqual(2, len(self.trader.strategy_states()))

    def test_get_strategy_states(self):
        # Arrange
        # Act
        status = self.trader.strategy_states()

        # Assert
        self.assertTrue(StrategyId("TradingStrategy-001") in status)
        self.assertTrue(StrategyId("TradingStrategy-002") in status)
        self.assertEqual("INITIALIZED", status[StrategyId("TradingStrategy-001")])
        self.assertEqual("INITIALIZED", status[StrategyId("TradingStrategy-002")])
        self.assertEqual(2, len(status))

    def test_change_strategies(self):
        # Arrange
        strategies = [
            TradingStrategy("003"),
            TradingStrategy("004"),
        ]

        # Act
        self.trader.initialize_strategies(strategies, warn_no_strategies=True)

        # Assert
        self.assertTrue(strategies[0].id in self.trader.strategy_states())
        self.assertTrue(strategies[1].id in self.trader.strategy_states())
        self.assertEqual(2, len(self.trader.strategy_states()))

    def test_trader_detects_duplicate_identifiers(self):
        # Arrange
        strategies = [
            TradingStrategy("000"),
            TradingStrategy("000"),
        ]

        # Act
        self.assertRaises(
            ValueError,
            self.trader.initialize_strategies,
            strategies,
            True,
        )

    def test_start_a_trader(self):
        # Arrange
        # Act
        self.trader.start()

        strategy_states = self.trader.strategy_states()

        # Assert
        self.assertEqual(ComponentState.RUNNING, self.trader.state)
        self.assertEqual("RUNNING", strategy_states[StrategyId("TradingStrategy-001")])
        self.assertEqual("RUNNING", strategy_states[StrategyId("TradingStrategy-002")])

    def test_stop_a_running_trader(self):
        # Arrange
        self.trader.start()

        # Act
        self.trader.stop()

        strategy_states = self.trader.strategy_states()

        # Assert
        self.assertEqual(ComponentState.STOPPED, self.trader.state)
        self.assertEqual("STOPPED", strategy_states[StrategyId("TradingStrategy-001")])
        self.assertEqual("STOPPED", strategy_states[StrategyId("TradingStrategy-002")])
