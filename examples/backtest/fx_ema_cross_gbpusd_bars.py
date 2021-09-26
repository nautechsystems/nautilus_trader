#!/usr/bin/env python3
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

import os
import sys
from decimal import Decimal

import pandas as pd


sys.path.insert(
    0, str(os.path.abspath(__file__ + "/../../../"))
)  # Allows relative imports from examples

from examples.strategies.ema_cross_simple import EMACross
from examples.strategies.ema_cross_simple import EMACrossConfig
from nautilus_trader.backtest.data.wranglers import QuoteTickDataWrangler
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider


if __name__ == "__main__":
    # Configure backtest engine
    config = BacktestEngineConfig(
        trader_id="BACKTESTER-001",
        log_level="INFO",
        use_data_cache=True,
        risk_engine={
            "bypass": True,  # Example of bypassing pre-trade risk checks for backtests
            "max_notional_per_order": {"GBP/USD.SIM": 2_000_000},
        },
    )
    # Build backtest engine
    engine = BacktestEngine(config=config)

    # Setup trading instruments
    SIM = Venue("SIM")
    GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD", SIM)

    # Setup data
    wrangler = QuoteTickDataWrangler(instrument=GBPUSD_SIM)
    ticks = wrangler.process_bar_data(
        bid_data=TestDataProvider.gbpusd_1min_bid(),
        ask_data=TestDataProvider.gbpusd_1min_ask(),
    )
    engine.add_instrument(GBPUSD_SIM)
    engine.add_ticks(ticks)

    # Create a fill model (optional)
    fill_model = FillModel(
        prob_fill_on_limit=0.2,
        prob_fill_on_stop=0.95,
        prob_slippage=0.5,
        random_seed=42,
    )

    # Optional plug in module to simulate rollover interest,
    # the data is coming from packaged test data.
    interest_rate_data = pd.read_csv(os.path.join(PACKAGE_ROOT, "data", "short-term-interest.csv"))
    fx_rollover_interest = FXRolloverInterestModule(rate_data=interest_rate_data)

    # Add an exchange (multiple exchanges possible)
    # Add starting balances for single-currency or multi-currency accounts
    engine.add_venue(
        venue=SIM,
        venue_type=VenueType.ECN,
        oms_type=OMSType.HEDGING,  # Venue will generate position_ids
        account_type=AccountType.MARGIN,
        base_currency=USD,  # Standard single-currency account
        starting_balances=[Money(10_000_000, USD)],
        fill_model=fill_model,
        modules=[fx_rollover_interest],
    )

    # Configure your strategy
    config = EMACrossConfig(
        instrument_id=str(GBPUSD_SIM.id),
        bar_type="GBP/USD.SIM-5-MINUTE-BID-INTERNAL",
        fast_ema_period=10,
        slow_ema_period=20,
        trade_size=Decimal(1_000_000),
        order_id_tag="001",
    )
    # Instantiate and add your strategy
    strategy = EMACross(config=config)
    engine.add_strategy(strategy=strategy)

    input("Press Enter to continue...")  # noqa (always Python 3)

    # Run the engine (from start to end of data)
    engine.run()

    # Optionally view reports
    with pd.option_context(
        "display.max_rows",
        100,
        "display.max_columns",
        None,
        "display.width",
        300,
    ):
        print(engine.trader.generate_account_report(SIM))
        print(engine.trader.generate_order_fills_report())
        print(engine.trader.generate_positions_report())

    # For repeated backtest runs make sure to reset the engine
    engine.reset()

    # Good practice to dispose of the object when done
    engine.dispose()
