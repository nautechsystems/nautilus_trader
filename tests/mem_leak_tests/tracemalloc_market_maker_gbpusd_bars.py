#!/usr/bin/env python3
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

import tracemalloc
from datetime import datetime
from decimal import Decimal

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.modules import FXRolloverInterestConfig
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.config.common import LoggingConfig
from nautilus_trader.examples.strategies.volatility_market_maker import VolatilityMarketMaker
from nautilus_trader.examples.strategies.volatility_market_maker import VolatilityMarketMakerConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def run_single_backtest():
    # Configure backtest engine
    config = BacktestEngineConfig(
        trader_id="BACKTESTER-001",
        logging=LoggingConfig(log_level="INFO", bypass_logging=True),
    )

    # Build the backtest engine
    engine = BacktestEngine(config=config)

    # Optional plug in module to simulate rollover interest,
    # the data is coming from packaged test data.
    provider = TestDataProvider()
    interest_rate_data = provider.read_csv("short-term-interest.csv")
    config = FXRolloverInterestConfig(interest_rate_data)
    fx_rollover_interest = FXRolloverInterestModule(config=config)

    # Create a fill model (optional)
    fill_model = FillModel(
        prob_fill_on_limit=0.2,
        prob_fill_on_stop=0.95,
        prob_slippage=0.5,
        random_seed=42,
    )

    # Add a trading venue (multiple venues possible)
    SIM = Venue("SIM")
    engine.add_venue(
        venue=SIM,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,  # Standard single-currency account
        starting_balances=[Money(10_000_000, USD)],  # Single-currency or multi-currency accounts
        fill_model=fill_model,
        modules=[fx_rollover_interest],
        bar_execution=True,  # If bar data should move the market (True by default)
    )

    # Add instruments
    GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD", SIM)
    engine.add_instrument(GBPUSD_SIM)

    # Add data
    wrangler = QuoteTickDataWrangler(GBPUSD_SIM)
    ticks = wrangler.process_bar_data(
        bid_data=provider.read_csv_bars("fxcm-gbpusd-m1-bid-2012.csv"),
        ask_data=provider.read_csv_bars("fxcm-gbpusd-m1-ask-2012.csv"),
    )
    engine.add_data(ticks)

    # Configure your strategy
    config = VolatilityMarketMakerConfig(
        instrument_id=str(GBPUSD_SIM.id),
        bar_type="GBP/USD.SIM-5-MINUTE-BID-INTERNAL",
        atr_period=20,
        atr_multiple=3.0,
        trade_size=Decimal(500_000),
        emulation_trigger="NO_TRIGGER",
    )
    # Instantiate and add your strategy
    strategy = VolatilityMarketMaker(config=config)
    engine.add_strategy(strategy=strategy)

    # Unnecessary for repeated runs
    # time.sleep(0.1)
    # input("Press Enter to continue...")

    # Run the engine (from start to end of data)
    engine.run(end=datetime(2012, 2, 10))

    # Optionally view reports
    # with pd.option_context(
    #     "display.max_rows",
    #     100,
    #     "display.max_columns",
    #     None,
    #     "display.width",
    #     300,
    # ):
    #     print(engine.trader.generate_account_report(SIM))
    #     print(engine.trader.generate_order_fills_report())
    #     print(engine.trader.generate_positions_report())

    # For repeated backtest runs make sure to reset the engine
    engine.reset()

    # Good practice to dispose of the object when done
    engine.dispose()


def run_n_backtests(n: int) -> None:
    for i in range(n):
        print(f"Running backtest {i}...")
        run_single_backtest()


if __name__ == "__main__":
    # Start tracemalloc
    tracemalloc.start()

    # Take initial snapshot of memory
    initial_memory_snapshot = tracemalloc.take_snapshot()

    # Run n backtests
    n = 10
    run_n_backtests(n)

    # Take final snapshot of memory
    final_memory_snapshot = tracemalloc.take_snapshot()

    # Compute difference and display statistics
    top_stats = final_memory_snapshot.compare_to(initial_memory_snapshot, "lineno")
    print("[ Top 10 differences ]")
    for stat in top_stats[:10]:
        print(stat)

    # Find and display largest memory block
    stat = top_stats[0]
    print(f"{stat.count} memory blocks: {stat.size / 1024:.1f} KiB")
    for line in stat.traceback.format():
        print(line)
