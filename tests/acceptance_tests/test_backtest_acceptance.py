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

from decimal import Decimal
import unittest

from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.data import BacktestDataContainer
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from tests.test_kit.data_provider import TestDataProvider
from tests.test_kit.strategies import EMACross
from tests.test_kit.strategies import EmptyStrategy


class BacktestAcceptanceTestsUSDJPYWithBars(unittest.TestCase):

    def setUp(self):
        self.venue = Venue("SIM")
        self.usdjpy = InstrumentLoader.default_fx_ccy(Symbol("USD/JPY", self.venue))
        data = BacktestDataContainer()
        data.add_instrument(self.usdjpy)
        data.add_bars(self.usdjpy.symbol, BarAggregation.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid())
        data.add_bars(self.usdjpy.symbol, BarAggregation.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask())

        config = BacktestConfig(
            tick_capacity=1000,
            bar_capacity=1000,
            exec_db_type='in-memory',
            exec_db_flush=False,
            frozen_account=False,
            starting_capital=1000000,
            account_currency=USD,
            short_term_interest_csv_path='default',
            bypass_logging=True,
            level_console=LogLevel.DEBUG,
            level_file=LogLevel.DEBUG,
            level_store=LogLevel.WARNING,
            log_thread=False,
            log_to_file=False,
        )

        self.engine = BacktestEngine(
            data=data,
            strategies=[EmptyStrategy('000')],
            venue=self.venue,
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            config=config,
        )

    def tearDown(self):
        self.engine.dispose()

    def test_run_ema_cross_strategy(self):
        # Arrange
        strategy = EMACross(
            symbol=self.usdjpy.symbol,
            bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
            trade_size=Decimal(1000000),
            fast_ema=10,
            slow_ema=20,
        )

        # Act
        self.engine.run(strategies=[strategy])

        # Assert - Should return expected PNL
        self.assertEqual(2689, strategy.fast_ema.count)
        self.assertEqual(115043, self.engine.iteration)
        self.assertEqual(Money(997688.53, USD), self.engine.portfolio.account(self.venue).balance())

    def test_rerun_ema_cross_strategy_returns_identical_performance(self):
        # Arrange
        strategy = EMACross(
            symbol=self.usdjpy.symbol,
            bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
            trade_size=Decimal(1000000),
            fast_ema=10,
            slow_ema=20,
        )

        self.engine.run(strategies=[strategy])
        result1 = self.engine.analyzer.get_performance_stats()

        # Act
        self.engine.reset()
        self.engine.run()
        result2 = self.engine.analyzer.get_performance_stats()

        # Assert
        self.assertEqual(all(result1), all(result2))

    def test_run_multiple_strategies(self):
        # Arrange
        strategy1 = EMACross(
            symbol=self.usdjpy.symbol,
            bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
            trade_size=Decimal(1000000),
            fast_ema=10,
            slow_ema=20,
            extra_id_tag='001',
        )

        strategy2 = EMACross(
            symbol=self.usdjpy.symbol,
            bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
            trade_size=Decimal(1000000),
            fast_ema=20,
            slow_ema=40,
            extra_id_tag='002',
        )

        # Note since these strategies are operating on the same symbol as per
        # the EMACross BUY/SELL logic they will be flattening each others positions.
        # The purpose of the test is just to ensure multiple strategies can run together.

        # Act
        self.engine.run(strategies=[strategy1, strategy2])

        # Assert
        self.assertEqual(2689, strategy1.fast_ema.count)
        self.assertEqual(2689, strategy2.fast_ema.count)
        self.assertEqual(115043, self.engine.iteration)
        self.assertEqual(Money(994553.21, USD), self.engine.portfolio.account(self.venue).balance())


class BacktestAcceptanceTestsGBPUSDWithBars(unittest.TestCase):

    def setUp(self):
        self.venue = Venue("SIM")
        self.gbpusd = InstrumentLoader.default_fx_ccy(Symbol("GBP/USD", self.venue))
        data = BacktestDataContainer()
        data.add_instrument(self.gbpusd)
        data.add_bars(self.gbpusd.symbol, BarAggregation.MINUTE, PriceType.BID, TestDataProvider.gbpusd_1min_bid())
        data.add_bars(self.gbpusd.symbol, BarAggregation.MINUTE, PriceType.ASK, TestDataProvider.gbpusd_1min_ask())

        config = BacktestConfig(
            tick_capacity=1000,
            bar_capacity=1000,
            exec_db_type='in-memory',
            exec_db_flush=False,
            frozen_account=False,
            starting_capital=1000000,
            account_currency=GBP,
            short_term_interest_csv_path='default',
            bypass_logging=True,
            level_console=LogLevel.DEBUG,
            level_file=LogLevel.DEBUG,
            level_store=LogLevel.WARNING,
            log_thread=False,
            log_to_file=False,
        )

        self.engine = BacktestEngine(
            data=data,
            strategies=[EmptyStrategy('000')],
            venue=self.venue,
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            config=config,
        )

    def tearDown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_bar_spec(self):
        # Arrange
        strategy = EMACross(
            symbol=self.gbpusd.symbol,
            bar_spec=BarSpecification(5, BarAggregation.MINUTE, PriceType.MID),
            trade_size=Decimal(1000000),
            fast_ema=10,
            slow_ema=20,
        )

        # Act
        self.engine.run(strategies=[strategy])

        # Assert
        self.assertEqual(8353, strategy.fast_ema.count)
        self.assertEqual(120467, self.engine.iteration)
        self.assertEqual(Money(947965.44, GBP), self.engine.portfolio.account(self.venue).balance())


class BacktestAcceptanceTestsAUDUSDWithTicks(unittest.TestCase):

    def setUp(self):
        self.venue = Venue("SIM")
        self.audusd = InstrumentLoader.default_fx_ccy(Symbol("AUD/USD", self.venue))
        data = BacktestDataContainer()
        data.add_instrument(self.audusd)
        data.add_quote_ticks(self.audusd.symbol, TestDataProvider.audusd_ticks())

        config = BacktestConfig(
            tick_capacity=1000,
            bar_capacity=1000,
            exec_db_type='in-memory',
            exec_db_flush=False,
            frozen_account=False,
            starting_capital=1000000,
            account_currency=AUD,  # Atypical account currency
            short_term_interest_csv_path='default',
            bypass_logging=True,
            level_console=LogLevel.DEBUG,
            level_file=LogLevel.DEBUG,
            level_store=LogLevel.WARNING,
            log_thread=False,
            log_to_file=False,
        )

        self.engine = BacktestEngine(
            data=data,
            strategies=[EmptyStrategy('000')],
            venue=self.venue,
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            config=config,
        )

    def tearDown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_bar_spec(self):
        # Arrange
        strategy = EMACross(
            symbol=self.audusd.symbol,
            bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.MID),
            trade_size=Decimal(1000000),
            fast_ema=10,
            slow_ema=20,
        )

        # Act
        self.engine.run(strategies=[strategy])

        # Assert
        self.assertEqual(1771, strategy.fast_ema.count)
        self.assertEqual(99999, self.engine.iteration)
        self.assertEqual(Money(991318.55, AUD), self.engine.portfolio.account(self.venue).balance())

    def test_run_ema_cross_with_tick_bar_spec(self):
        # Arrange
        strategy = EMACross(
            symbol=self.audusd.symbol,
            bar_spec=BarSpecification(100, BarAggregation.TICK, PriceType.MID),
            trade_size=Decimal(1000000),
            fast_ema=10,
            slow_ema=20,
        )

        # Act
        self.engine.run(strategies=[strategy])

        # Assert
        self.assertEqual(999, strategy.fast_ema.count)
        self.assertEqual(99999, self.engine.iteration)
        self.assertEqual(Money(995390.28, AUD), self.engine.portfolio.account(self.venue).balance())
