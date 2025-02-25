#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd
from strategy import DemoStrategy
from strategy import DemoStrategyConfig

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model import TraderId
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


if __name__ == "__main__":

    # Step 1: Configure and create backtest engine
    engine_config = BacktestEngineConfig(
        trader_id=TraderId("BACKTEST_TRADER-001"),
        logging=LoggingConfig(
            log_level="DEBUG",  # Enable debug logging
        ),
    )
    engine = BacktestEngine(config=engine_config)

    # Step 2: Define exchange venue and add it to the engine
    # We use XCME (CME Exchange) and configure it with margin account
    XCME = Venue("XCME")
    engine.add_venue(
        venue=XCME,
        oms_type=OmsType.NETTING,  # Order Management System type
        account_type=AccountType.MARGIN,  # Type of trading account
        starting_balances=[Money(1_000_000, USD)],  # Initial account balance
        base_currency=USD,  # Base currency for account
        default_leverage=Decimal(1),  # No leverage used for account
    )

    # Step 3: Create instrument definition and add it to the engine
    # We use EURUSD futures contract for this example
    EURUSD_INSTRUMENT = TestInstrumentProvider.eurusd_future(
        expiry_year=2024,
        expiry_month=3,
        venue_name="XCME",
    )
    engine.add_instrument(EURUSD_INSTRUMENT)

    # Step 4: Load and prepare market data

    # Step 4a: Load bar data from CSV file -> into pandas DataFrame
    csv_file_path = rf"{TEST_DATA_DIR}/xcme/6EH4.XCME_1min_bars_20240101_20240131.csv.gz"
    df = pd.read_csv(csv_file_path, header=0, index_col=False)

    # Step 4b: Restructure DataFrame into required format
    # Restructure DataFrame into required format
    df = (
        # Reorder columns to match required format
        df.reindex(columns=["timestamp_utc", "open", "high", "low", "close", "volume"])
        # Convert timestamp strings to datetime objects
        .assign(
            timestamp_utc=lambda dft: pd.to_datetime(
                dft["timestamp_utc"],
                format="%Y-%m-%d %H:%M:%S",
            ),
        )
        # Rename timestamp column and set as index
        .rename(columns={"timestamp_utc": "timestamp"}).set_index("timestamp")
    )

    # Step 4c: Define bar type for our data
    EURUSD_1MIN_BARTYPE = BarType.from_str(f"{EURUSD_INSTRUMENT.id}-1-MINUTE-LAST-EXTERNAL")

    # Step 4d: Convert DataFrame rows into Bar objects
    wrangler = BarDataWrangler(EURUSD_1MIN_BARTYPE, EURUSD_INSTRUMENT)
    eurusd_1min_bars_list: list[Bar] = wrangler.process(df)

    # Step 4e: Add the prepared data to the engine
    engine.add_data(eurusd_1min_bars_list)

    # Step 5: Create and add our portfolio demonstration strategy
    strategy_config = DemoStrategyConfig(
        bar_type=EURUSD_1MIN_BARTYPE,
        instrument=EURUSD_INSTRUMENT,
    )
    strategy = DemoStrategy(config=strategy_config)
    engine.add_strategy(strategy)

    # Step 6: Run the backtest
    engine.run()

    # Step 7: Release system resources
    engine.dispose()
