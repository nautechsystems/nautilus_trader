#!/usr/bin/env python3
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
import os
import time

import pandas as pd

from examples.strategies.ema_cross_simple import EMACross
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.data_container import BacktestDataContainer
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.data_provider import TestDataProvider


if __name__ == "__main__":
    # Setup trading instruments
    SIM = Venue("SIM")
    symbol = Symbol("AUD/USD", SIM)
    AUDUSD = InstrumentLoader.default_fx_ccy(symbol)

    # Setup data container
    data = BacktestDataContainer()
    data.add_instrument(AUDUSD)
    data.add_quote_ticks(
        symbol=AUDUSD.symbol,
        data=TestDataProvider.audusd_ticks(),  # Stub data from the test kit
    )

    # Instantiate your strategy
    strategy = EMACross(
        symbol=AUDUSD.symbol,
        bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.MID),
        fast_ema=10,
        slow_ema=20,
        trade_size=Decimal(1000000),
    )

    time.sleep(0.1)  # Allow strategy initialization to log

    # Customize the backtest configuration (optional)
    config = BacktestConfig(
        exec_db_type="in-memory",
        exec_db_flush=False,
        frozen_account=False,
        starting_capital=1000000,
        account_currency=USD,
        short_term_interest_csv_path="default",
        bypass_logging=False,
        level_console=LogLevel.INFO,
        level_file=LogLevel.DEBUG,
        level_store=LogLevel.WARNING,
        log_thread=False,
        log_to_file=False,
    )

    # Create a fill model (optional)
    fill_model = FillModel(
        prob_fill_at_limit=0.2,
        prob_fill_at_stop=0.95,
        prob_slippage=0.5,
        random_seed=42,
    )

    # Build the backtest engine
    engine = BacktestEngine(
        data=data,
        trader_id=TraderId("BACKTESTER", "000"),
        strategies=[strategy],  # List of `any` number of strategies
        config=config,
    )

    # Optional plug in module to simulate rollover interest,
    # the data is coming from packaged test data.
    interest_rate_data = pd.read_csv(os.path.join(PACKAGE_ROOT + "/data/", "short-term-interest.csv"))
    fx_rollover_interest = FXRolloverInterestModule(rate_data=interest_rate_data)

    # Add exchange (now multiple exchanges possible)
    engine.add_exchange(
        venue=SIM,
        oms_type=OMSType.HEDGING,
        generate_position_ids=False,
        fill_model=fill_model,
        modules=[fx_rollover_interest],
    )

    input("Press Enter to continue...")  # noqa (always Python 3)

    # Run the engine from start to end of data
    engine.run()

    # Optionally view reports
    with pd.option_context(
            "display.max_rows",
            100,
            "display.max_columns",
            None,
            "display.width", 300):
        print(engine.trader.generate_account_report(AccountId.from_str("SIM-000-SIMULATED")))
        print(engine.trader.generate_order_fills_report())
        print(engine.trader.generate_positions_report())

    # Good practice to dispose of the object
    engine.dispose()
