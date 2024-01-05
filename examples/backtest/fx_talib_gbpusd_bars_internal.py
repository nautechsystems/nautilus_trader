#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import time

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.modules import FXRolloverInterestConfig
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.config.common import LoggingConfig
from nautilus_trader.config.common import RiskEngineConfig
from nautilus_trader.examples.strategies.talib_strategy import TALibStrategy
from nautilus_trader.examples.strategies.talib_strategy import TALibStrategyConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


if __name__ == "__main__":
    # Configure backtest engine
    config = BacktestEngineConfig(
        trader_id="BACKTESTER-001",
        logging=LoggingConfig(log_level="INFO"),
        risk_engine=RiskEngineConfig(
            bypass=True,  # Example of bypassing pre-trade risk checks for backtests
        ),
    )

    # Build backtest engine
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
        oms_type=OmsType.HEDGING,  # Venue will generate position IDs
        account_type=AccountType.MARGIN,
        base_currency=USD,  # Standard single-currency account
        starting_balances=[Money(1_000_000, USD)],  # Single-currency or multi-currency accounts
        fill_model=fill_model,
        modules=[fx_rollover_interest],
        bar_execution=True,  # If bar data should move the market (True by default)
    )

    # Add instruments
    GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD", SIM)
    engine.add_instrument(GBPUSD_SIM)

    # Add data
    wrangler = QuoteTickDataWrangler(instrument=GBPUSD_SIM)
    ticks = wrangler.process_bar_data(
        bid_data=provider.read_csv_bars("fxcm/gbpusd-m1-bid-2012.csv"),
        ask_data=provider.read_csv_bars("fxcm/gbpusd-m1-ask-2012.csv"),
    )
    engine.add_data(ticks)

    # Configure your strategy
    config = TALibStrategyConfig(
        bar_type=BarType.from_str("GBP/USD.SIM-5-MINUTE-BID-INTERNAL"),
    )

    # Instantiate and add your strategy
    strategy = TALibStrategy(config=config)
    engine.add_strategy(strategy=strategy)

    time.sleep(0.1)
    input("Press Enter to continue...")

    # Run the engine (from start to end of data)
    engine.run()

    # For repeated backtest runs make sure to reset the engine
    engine.reset()

    # Good practice to dispose of the object when done
    engine.dispose()
