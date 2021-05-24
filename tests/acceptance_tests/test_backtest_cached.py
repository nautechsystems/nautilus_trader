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

from decimal import Decimal
import os
import unittest

import pandas as pd

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.strategies import EMACross


class BacktestAcceptanceTestsUSDJPYWithBars(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.engine = BacktestEngine(
            bypass_logging=True,
            use_data_cache=True,
        )

        self.venue = Venue("SIM")
        self.usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

        self.engine.add_instrument(self.usdjpy)
        self.engine.add_bars(
            self.usdjpy.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid(),
        )
        self.engine.add_bars(
            self.usdjpy.id,
            BarAggregation.MINUTE,
            PriceType.ASK,
            TestDataProvider.usdjpy_1min_ask(),
        )

        interest_rate_data = pd.read_csv(
            os.path.join(PACKAGE_ROOT, "data", "short-term-interest.csv")
        )
        fx_rollover_interest = FXRolloverInterestModule(rate_data=interest_rate_data)

        self.engine.add_venue(
            venue=self.venue,
            venue_type=VenueType.ECN,
            oms_type=OMSType.HEDGING,
            starting_balances=[Money(1_000_000, USD)],
            modules=[fx_rollover_interest],
        )

    def tearDown(self):
        self.engine.dispose()

    def test_run_ema_cross_strategy(self):
        # Arrange
        strategy = EMACross(
            instrument_id=self.usdjpy.id,
            bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )

        # Act
        self.engine.run(strategies=[strategy])

        # Assert - Should return expected PnL
        self.assertEqual(2689, strategy.fast_ema.count)
        self.assertEqual(115043, self.engine.iteration)
        self.assertEqual(
            Money(997731.23, USD), self.engine.portfolio.account(self.venue).balance()
        )

    def test_rerun_ema_cross_strategy_returns_identical_performance(self):
        # Arrange
        strategy = EMACross(
            instrument_id=self.usdjpy.id,
            bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )

        self.engine.run(strategies=[strategy])
        result1 = self.engine.analyzer.get_performance_stats_pnls()

        # Act
        self.engine.reset()
        self.engine.run()
        result2 = self.engine.analyzer.get_performance_stats_pnls()

        # Assert
        self.assertEqual(all(result1), all(result2))

    def test_run_multiple_strategies(self):
        # Arrange
        strategy1 = EMACross(
            instrument_id=self.usdjpy.id,
            bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
            extra_id_tag="001",
        )

        strategy2 = EMACross(
            instrument_id=self.usdjpy.id,
            bar_spec=BarSpecification(15, BarAggregation.MINUTE, PriceType.BID),
            trade_size=Decimal(1_000_000),
            fast_ema=20,
            slow_ema=40,
            extra_id_tag="002",
        )

        # Note since these strategies are operating on the same instrument_id as per
        # the EMACross BUY/SELL logic they will be flattening each others positions.
        # The purpose of the test is just to ensure multiple strategies can run together.

        # Act
        self.engine.run(strategies=[strategy1, strategy2])

        # Assert
        self.assertEqual(2689, strategy1.fast_ema.count)
        self.assertEqual(2689, strategy2.fast_ema.count)
        self.assertEqual(115043, self.engine.iteration)
        self.assertEqual(
            Money(992818.88, USD), self.engine.portfolio.account(self.venue).balance()
        )


class BacktestAcceptanceTestsGBPUSDWithBars(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.engine = BacktestEngine(
            bypass_logging=True,
            use_data_cache=True,
        )

        self.venue = Venue("SIM")
        self.gbpusd = TestInstrumentProvider.default_fx_ccy("GBP/USD")

        self.engine.add_instrument(self.gbpusd)
        self.engine.add_bars(
            self.gbpusd.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.gbpusd_1min_bid(),
        )
        self.engine.add_bars(
            self.gbpusd.id,
            BarAggregation.MINUTE,
            PriceType.ASK,
            TestDataProvider.gbpusd_1min_ask(),
        )

        interest_rate_data = pd.read_csv(
            os.path.join(PACKAGE_ROOT, "data", "short-term-interest.csv")
        )
        fx_rollover_interest = FXRolloverInterestModule(rate_data=interest_rate_data)

        self.engine.add_venue(
            venue=self.venue,
            venue_type=VenueType.ECN,
            oms_type=OMSType.HEDGING,
            starting_balances=[Money(1_000_000, GBP)],
            modules=[fx_rollover_interest],
        )

    def tearDown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_bar_spec(self):
        # Arrange
        strategy = EMACross(
            instrument_id=self.gbpusd.id,
            bar_spec=BarSpecification(5, BarAggregation.MINUTE, PriceType.MID),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )

        # Act
        self.engine.run(strategies=[strategy])

        # Assert
        self.assertEqual(8353, strategy.fast_ema.count)
        self.assertEqual(120467, self.engine.iteration)
        self.assertEqual(
            Money(947226.84, GBP), self.engine.portfolio.account(self.venue).balance()
        )


class BacktestAcceptanceTestsAUDUSDWithTicks(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.engine = BacktestEngine(
            bypass_logging=True,
            use_data_cache=True,
        )

        self.venue = Venue("SIM")
        self.audusd = TestInstrumentProvider.default_fx_ccy("AUD/USD")

        self.engine.add_instrument(self.audusd)
        self.engine.add_quote_ticks(self.audusd.id, TestDataProvider.audusd_ticks())

        interest_rate_data = pd.read_csv(
            os.path.join(PACKAGE_ROOT, "data", "short-term-interest.csv")
        )
        fx_rollover_interest = FXRolloverInterestModule(rate_data=interest_rate_data)

        self.engine.add_venue(
            venue=self.venue,
            venue_type=VenueType.ECN,
            oms_type=OMSType.HEDGING,
            starting_balances=[Money(1_000_000, AUD)],
            modules=[fx_rollover_interest],
        )

    def tearDown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_minute_bar_spec(self):
        # Arrange
        strategy = EMACross(
            instrument_id=self.audusd.id,
            bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.MID),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )

        # Act
        self.engine.run(strategies=[strategy])

        # Assert
        self.assertEqual(1771, strategy.fast_ema.count)
        self.assertEqual(99999, self.engine.iteration)
        self.assertEqual(
            Money(991360.19, AUD), self.engine.portfolio.account(self.venue).balance()
        )

    def test_run_ema_cross_with_tick_bar_spec(self):
        # Arrange
        strategy = EMACross(
            instrument_id=self.audusd.id,
            bar_spec=BarSpecification(100, BarAggregation.TICK, PriceType.MID),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )

        # Act
        self.engine.run(strategies=[strategy])

        # Assert
        self.assertEqual(999, strategy.fast_ema.count)
        self.assertEqual(99999, self.engine.iteration)
        self.assertEqual(
            Money(995431.92, AUD), self.engine.portfolio.account(self.venue).balance()
        )


class BacktestAcceptanceTestsETHUSDTWithTrades(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.engine = BacktestEngine(
            bypass_logging=True,
            use_data_cache=True,
        )

        self.venue = Venue("BINANCE")
        self.ethusdt = TestInstrumentProvider.ethusdt_binance()

        self.engine.add_instrument(self.ethusdt)
        self.engine.add_trade_ticks(self.ethusdt.id, TestDataProvider.ethusdt_trades())

        self.engine.add_venue(
            venue=self.venue,
            venue_type=VenueType.EXCHANGE,
            oms_type=OMSType.NETTING,
            starting_balances=[Money(1_000_000, USDT)],
        )

    def tearDown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_tick_bar_spec(self):
        # Arrange
        strategy = EMACross(
            instrument_id=self.ethusdt.id,
            bar_spec=BarSpecification(250, BarAggregation.TICK, PriceType.LAST),
            trade_size=Decimal(100),
            fast_ema=10,
            slow_ema=20,
        )

        # Act
        self.engine.run(strategies=[strategy])

        # Assert
        self.assertEqual(279, strategy.fast_ema.count)
        self.assertEqual(69806, self.engine.iteration)
        self.assertEqual(
            Money(997703.75635820, USDT),
            self.engine.portfolio.account(self.venue).balance(),
        )


class BacktestAcceptanceTestsBTCUSDTWithTradesAndQuotes(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.engine = BacktestEngine(
            bypass_logging=True,
            use_data_cache=True,
        )

        self.venue = Venue("BINANCE")
        self.instrument = TestInstrumentProvider.btcusdt_binance()

        self.engine.add_instrument(self.instrument)
        self.engine.add_trade_ticks(
            self.instrument.id, TestDataProvider.tardis_trades()
        )
        self.engine.add_quote_ticks(
            self.instrument.id, TestDataProvider.tardis_quotes()
        )
        self.engine.add_venue(
            venue=self.venue,
            venue_type=VenueType.EXCHANGE,
            oms_type=OMSType.NETTING,
            starting_balances=[Money(1_000_000, USDT)],
        )

    def tearDown(self):
        self.engine.dispose()

    def test_run_ema_cross_with_tick_bar_spec(self):
        # Arrange
        strategy = EMACross(
            instrument_id=self.instrument.id,
            bar_spec=BarSpecification(250, BarAggregation.TICK, PriceType.LAST),
            trade_size=Decimal(100),
            fast_ema=10,
            slow_ema=20,
        )

        # Act
        self.engine.run(strategies=[strategy])

        # Assert
        self.assertEqual(39, strategy.fast_ema.count)
        self.assertEqual(19998, self.engine.iteration)
        self.assertEqual(
            Money(991976.84796000, USDT),
            self.engine.portfolio.account(self.venue).balance(),
        )
