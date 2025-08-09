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

import pandas as pd
from strategy import DemoStrategy
from strategy import DemoStrategyConfig

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.common.config import LoggingConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


if __name__ == "__main__":
    # ==========================================================================================
    # POINT OF FOCUS: Loading bars from CSV into data catalog for BacktestNode
    # ------------------------------------------------------------------------------------------

    # Step 1: Create instrument definition
    EURUSD_FUTURES_INSTRUMENT = TestInstrumentProvider.eurusd_future(
        expiry_year=2024,
        expiry_month=3,
        venue_name="XCME",
    )

    # Step 2: Load bar data from CSV file -> into pandas DataFrame
    csv_file_path = r"6EH4.XCME_1min_bars.csv"
    df = pd.read_csv(csv_file_path, sep=";", decimal=".", header=0, index_col=False)

    # Step 3: Restructure DataFrame into required structure for BarDataWrangler
    #   - 5 columns: 'open', 'high', 'low', 'close', 'volume' (volume is optional)
    #   - 'timestamp' as index

    # Change order of columns
    df = df.reindex(columns=["timestamp_utc", "open", "high", "low", "close", "volume"])
    # Convert string timestamps into datetime
    df["timestamp_utc"] = pd.to_datetime(df["timestamp_utc"], format="%Y-%m-%d %H:%M:%S")
    # Rename column to required name
    df = df.rename(columns={"timestamp_utc": "timestamp"})
    # Set column `timestamp` as index
    df = df.set_index("timestamp")

    # Step 4: Define type of loaded bars
    EURUSD_FUTURES_1MIN_BARTYPE = BarType.from_str(
        f"{EURUSD_FUTURES_INSTRUMENT.id}-1-MINUTE-LAST-EXTERNAL",
    )

    # Step 5: Convert DataFrame rows into Bar objects using BarDataWrangler
    wrangler = BarDataWrangler(EURUSD_FUTURES_1MIN_BARTYPE, EURUSD_FUTURES_INSTRUMENT)
    eurusd_1min_bars_list: list[Bar] = wrangler.process(df)

    # Step 6: Create data catalog and write data to it
    catalog_path = "./data_catalog_example_01"
    data_catalog = ParquetDataCatalog(catalog_path)

    # Write instrument and bar data to catalog
    data_catalog.write_data([EURUSD_FUTURES_INSTRUMENT])  # Store instrument definition
    data_catalog.write_data(eurusd_1min_bars_list)  # Store bar data

    # ------------------------------------------------------------------------------------------
    # END OF POINT OF FOCUS
    # ==========================================================================================

    # Step 7: Configure BacktestNode with high-level API

    # Configure strategy using ImportableStrategyConfig
    strategies = [
        ImportableStrategyConfig(
            strategy_path=DemoStrategy.fully_qualified_name(),
            config_path=DemoStrategyConfig.fully_qualified_name(),
            config={
                "primary_bar_type": str(EURUSD_FUTURES_1MIN_BARTYPE),
            },
        ),
    ]

    # Configure logging
    logging = LoggingConfig(
        log_level="DEBUG",  # Set DEBUG log level for console to see loaded bars in logs
    )

    # Configure backtest engine
    engine_config = BacktestEngineConfig(
        trader_id=TraderId("BACKTEST_TRADER-001"),
        logging=logging,
        strategies=strategies,
    )

    # Configure venue
    venues = [
        BacktestVenueConfig(
            name="XCME",
            oms_type="NETTING",  # Order Management System type
            account_type="MARGIN",  # Type of trading account
            starting_balances=["1_000_000 USD"],  # Initial account balance
            base_currency="USD",  # Base currency for account
            default_leverage=1.0,  # No leverage used for account
        ),
    ]

    # Configure data source from catalog
    data = [
        BacktestDataConfig(
            catalog_path=catalog_path,
            data_cls=Bar,
            instrument_id=EURUSD_FUTURES_INSTRUMENT.id,
        ),
    ]

    # Create BacktestRunConfig
    run_config = BacktestRunConfig(
        engine=engine_config,
        venues=venues,
        data=data,
    )

    # Step 8: Create and run BacktestNode
    node = BacktestNode(configs=[run_config])

    # Run the backtest
    results = node.run()

    # Step 9: Clean up resources
    node.dispose()
