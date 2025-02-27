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
from strategy import CacheDemoStrategy

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import CacheConfig
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
    # Configure backtest engine with Cache settings
    engine_config = BacktestEngineConfig(
        trader_id=TraderId("BACKTEST-001"),
        logging=LoggingConfig(
            log_level="INFO",  # Show Cache operations in logs
        ),
        cache=CacheConfig(
            bar_capacity=5000,  # Store last 5000 bars per bar type
            tick_capacity=10000,  # Store last 10000 ticks per instrument (not used in this strategy)
        ),
    )

    # Create backtest engine
    engine = BacktestEngine(config=engine_config)

    # Set up a simple trading environment
    venue = Venue("SIM")

    # Add a trading venue
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
    )

    # Step 2: Define exchange and add it to the engine
    venue_name = "XCME"
    engine.add_venue(
        venue=Venue(venue_name),
        oms_type=OmsType.NETTING,  # Order Management System type
        account_type=AccountType.MARGIN,  # Type of trading account
        starting_balances=[Money(1_000_000, USD)],  # Initial account balance
        base_currency=USD,  # Base currency for account
        default_leverage=Decimal(1),  # No leverage used for account
    )

    # Step 3: Create instrument definition and add it to the engine
    EURUSD_FUTURES = TestInstrumentProvider.eurusd_future(
        expiry_year=2024,
        expiry_month=3,
        venue_name=venue_name,
    )
    engine.add_instrument(EURUSD_FUTURES)

    # Loading bars from CSV

    # Step 4a: Load bar data from CSV file -> into pandas DataFrame
    csv_file_path = rf"{TEST_DATA_DIR}/xcme/6EH4.XCME_1min_bars_20240101_20240131.csv.gz"
    df = pd.read_csv(csv_file_path, header=0, index_col=False)

    # Step 4b: Restructure DataFrame into required structure for BarDataWrangler
    #   - 5 required columns: 'open', 'high', 'low', 'close', 'volume' (volume is optional)
    #   - column 'timestamp': should be in index of the DataFrame
    df = df.reindex(columns=["timestamp_utc", "open", "high", "low", "close", "volume"])
    df["timestamp_utc"] = pd.to_datetime(df["timestamp_utc"], format="%Y-%m-%d %H:%M:%S")
    df = df.rename(columns={"timestamp_utc": "timestamp"})
    df = df.set_index("timestamp")

    # Step 4c: Define type of loaded bars
    bar_type = BarType.from_str(f"{EURUSD_FUTURES.id}-1-MINUTE-LAST-EXTERNAL")

    # Step 4d: Convert DataFrame rows into Bar objects
    wrangler = BarDataWrangler(bar_type, EURUSD_FUTURES)
    bars: list[Bar] = wrangler.process(df)

    # Step 4e: Add loaded data to the engine
    engine.add_data(bars)

    # ------------------------------------------------------------------------------------------

    # Step 5: Create strategy and add it to the engine
    strategy = CacheDemoStrategy(bar_type=bar_type)
    engine.add_strategy(strategy)

    # Step 6: Run engine = Run backtest
    engine.run()

    # Step 7: Release system resources
    engine.dispose()
