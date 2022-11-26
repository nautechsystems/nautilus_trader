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

import os
from datetime import datetime
from decimal import Decimal

import pandas as pd
import pytz

from nautilus_trader.backtest.data.providers import TestDataProvider
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.data.wranglers import QuoteTickDataWrangler
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.trading.strategy import Strategy
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.performance import PerformanceHarness
from tests.test_kit.stubs.data import TestDataStubs


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestBacktestEnginePerformance(PerformanceHarness):
    @staticmethod
    def test_run_with_empty_strategy(benchmark):
        def setup():
            # Arrange
            config = BacktestEngineConfig(bypass_logging=True)
            engine = BacktestEngine(config=config)

            engine.add_venue(
                venue=Venue("SIM"),
                oms_type=OMSType.HEDGING,
                account_type=AccountType.MARGIN,
                base_currency=USD,
                starting_balances=[Money(1_000_000, USD)],
                fill_model=FillModel(),
            )

            engine.add_instrument(USDJPY_SIM)

            # Setup data
            wrangler = QuoteTickDataWrangler(USDJPY_SIM)
            provider = TestDataProvider()
            ticks = wrangler.process_bar_data(
                bid_data=provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv"),
                ask_data=provider.read_csv_bars("fxcm-usdjpy-m1-ask-2013.csv"),
            )
            engine.add_data(ticks)

            strategies = [Strategy()]
            start = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=pytz.utc)
            end = datetime(2013, 8, 10, 0, 0, 0, 0, tzinfo=pytz.utc)
            return (engine, start, end, strategies), {}

        def run(engine, start, end, strategies):
            engine.add_strategies(strategies=strategies)
            engine.run(start=start, end=end)

        benchmark.pedantic(run, setup=setup, rounds=1, iterations=1, warmup_rounds=1)

    @staticmethod
    def test_run_for_tick_processing(benchmark):
        def setup():
            config = BacktestEngineConfig(bypass_logging=True)
            engine = BacktestEngine(config=config)

            engine.add_venue(
                venue=Venue("SIM"),
                oms_type=OMSType.HEDGING,
                account_type=AccountType.MARGIN,
                base_currency=USD,
                starting_balances=[Money(1_000_000, USD)],
            )

            engine.add_instrument(USDJPY_SIM)

            # Setup data
            wrangler = QuoteTickDataWrangler(USDJPY_SIM)
            provider = TestDataProvider()
            ticks = wrangler.process_bar_data(
                bid_data=provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv"),
                ask_data=provider.read_csv_bars("fxcm-usdjpy-m1-ask-2013.csv"),
            )
            engine.add_data(ticks)

            config = EMACrossConfig(
                instrument_id=str(USDJPY_SIM.id),
                bar_type=str(TestDataStubs.bartype_usdjpy_1min_bid()),
                trade_size=Decimal(1_000_000),
                fast_ema=10,
                slow_ema=20,
            )
            strategy = EMACross(config=config)

            start = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=pytz.utc)
            end = datetime(2013, 2, 10, 0, 0, 0, 0, tzinfo=pytz.utc)

            return (engine, start, end, strategy), {}

        def run(engine, start, end, strategy):
            engine.add_strategy(strategy)
            engine.run(start=start, end=end)

        benchmark.pedantic(run, setup=setup, rounds=1, iterations=1)

    @staticmethod
    def test_run_with_ema_cross_strategy(benchmark):
        def setup():
            config = BacktestEngineConfig(bypass_logging=True)
            engine = BacktestEngine(config=config)

            provider = TestDataProvider()
            interest_rate_data = pd.read_csv(
                os.path.join(PACKAGE_ROOT, "data", "short-term-interest.csv"),
            )

            fx_rollover_interest = FXRolloverInterestModule(rate_data=interest_rate_data)

            engine.add_venue(
                venue=Venue("SIM"),
                oms_type=OMSType.HEDGING,
                account_type=AccountType.MARGIN,
                base_currency=USD,
                starting_balances=[Money(1_000_000, USD)],
                modules=[fx_rollover_interest],
            )

            engine.add_instrument(USDJPY_SIM)

            # Setup data
            wrangler = QuoteTickDataWrangler(USDJPY_SIM)
            ticks = wrangler.process_bar_data(
                bid_data=provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv"),
                ask_data=provider.read_csv_bars("fxcm-usdjpy-m1-ask-2013.csv"),
            )
            engine.add_data(ticks)

            config = EMACrossConfig(
                instrument_id=str(USDJPY_SIM.id),
                bar_type=str(TestDataStubs.bartype_usdjpy_1min_bid()),
                trade_size=Decimal(1_000_000),
                fast_ema=10,
                slow_ema=20,
            )
            strategy = EMACross(config=config)

            start = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=pytz.utc)
            end = datetime(2013, 3, 1, 0, 0, 0, 0, tzinfo=pytz.utc)

            return (engine, start, end, [strategy]), {}

        def run(engine, start, end, strategies):
            engine.add_strategies(strategies)
            engine.run(start=start, end=end)

        benchmark.pedantic(run, setup=setup, rounds=1, iterations=1)
