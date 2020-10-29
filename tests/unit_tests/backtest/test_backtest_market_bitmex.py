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

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.exchange import SimulatedExchange
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.uuid import TestUUIDFactory
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.database import BypassExecutionDatabase
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.strategies import TestStrategy
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH

XBTUSD_BITMEX = InstrumentLoader.xbtusd_bitmex()


class BinanceSimulatedMarketTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.strategies = [TestStrategy(TestStubs.bartype_btcusdt_binance_1min_bid())]

        self.clock = TestClock()
        self.uuid_factory = TestUUIDFactory()
        self.logger = TestLogger(self.clock)

        self.portfolio = Portfolio(
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.portfolio.update_instrument(XBTUSD_BITMEX)

        self.data_engine = DataEngine(
            portfolio=self.portfolio,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
            config={'use_previous_close': False},  # To correctly reproduce historical data bars
        )

        self.analyzer = PerformanceAnalyzer()

        self.trader_id = TraderId("TESTER", "000")
        self.account_id = AccountId("BITMEX", "001", AccountType.SIMULATED)

        exec_db = BypassExecutionDatabase(
            trader_id=self.trader_id,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            database=exec_db,
            portfolio=self.portfolio,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.config = BacktestConfig()
        self.exchange = SimulatedExchange(
            venue=Venue("BITMEX"),
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            exec_cache=self.exec_engine.cache,
            instruments={XBTUSD_BITMEX.symbol: XBTUSD_BITMEX},
            config=self.config,
            fill_model=FillModel(),
            clock=self.clock,
            uuid_factory=TestUUIDFactory(),
            logger=self.logger,
        )

        self.exec_client = BacktestExecClient(
            market=self.exchange,
            account_id=self.account_id,
            engine=self.exec_engine,
            logger=self.logger,
        )

        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.strategy = TestStrategy(bar_type=TestStubs.bartype_btcusdt_binance_1min_bid())
        self.strategy.register_trader(
            self.trader_id,
            self.clock,
            self.uuid_factory,
            self.logger,
        )

        self.data_engine.register_strategy(self.strategy)
        self.exec_engine.register_strategy(self.strategy)

    def test_commission_maker_taker_order(self):
        # Arrange
        self.strategy.start()

        quote1 = QuoteTick(
            XBTUSD_BITMEX.symbol,
            Price("11493.70"),
            Price("11493.75"),
            Quantity(1500000),
            Quantity(1500000),
            UNIX_EPOCH,
        )

        self.exchange.process_tick(quote1)  # Prepare market
        self.portfolio.update_tick(quote1)

        order_market = self.strategy.order_factory.market(
            XBTUSD_BITMEX.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        order_limit = self.strategy.order_factory.limit(
            XBTUSD_BITMEX.symbol,
            OrderSide.BUY,
            Quantity(100000),
            Price("11493.65"),
        )

        # Act
        self.strategy.submit_order(order_market)
        self.strategy.submit_order(order_limit)

        quote2 = QuoteTick(
            XBTUSD_BITMEX.symbol,
            Price("11493.60"),
            Price("11493.65"),
            Quantity(1500000),
            Quantity(1500000),
            UNIX_EPOCH,
        )

        self.exchange.process_tick(quote2)  # Fill the limit order
        self.portfolio.update_tick(quote2)

        # Assert
        self.assertEqual(LiquiditySide.TAKER, self.strategy.object_storer.get_store()[3].liquidity_side)
        self.assertEqual(LiquiditySide.MAKER, self.strategy.object_storer.get_store()[8].liquidity_side)
        self.assertEqual(Money(0.00652529, BTC), self.strategy.object_storer.get_store()[3].commission)
        self.assertEqual(Money(-0.00217511, BTC), self.strategy.object_storer.get_store()[8].commission)
