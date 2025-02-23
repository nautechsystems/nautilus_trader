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

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
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
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


log = Logger(__name__)


if __name__ == "__main__":

    # ----------------------------------------------------
    # Step 1: Configure and Create the Backtest Engine
    # ----------------------------------------------------
    engine_config = BacktestEngineConfig(
        trader_id=TraderId("BACKTEST_TRADER-001"),
        logging=LoggingConfig(log_level="DEBUG"),  # required to show detailed logs
    )
    engine = BacktestEngine(config=engine_config)

    # ----------------------------------------------------
    # Step 2: Define Venue and Add it to the Engine
    # ----------------------------------------------------
    venue_name = "XCME"
    engine.add_venue(
        venue=Venue(venue_name),
        oms_type=OmsType.NETTING,  # Netting: positions are netted against each other
        account_type=AccountType.MARGIN,  # Margin account: allows trading with leverage
        starting_balances=[Money(1_000_000, USD)],  # Initial account balance of $1,000,000 USD
        base_currency=USD,  # Account base currency is USD
        default_leverage=Decimal(1),  # No leverage is used (1:1)
    )

    # ----------------------------------------------------
    # Step 3: Define Instrument and Add it to the Engine
    # ----------------------------------------------------

    eurusd_futures = TestInstrumentProvider.eurusd_future(
        expiry_year=2024,
        expiry_month=3,
        venue_name=venue_name,
    )
    engine.add_instrument(eurusd_futures)

    # ----------------------------------------------------
    # Step 4: Load Historical Bar Data and Add to Engine
    # ----------------------------------------------------

    # Step 4a: Load bar data from CSV file -> into pandas DataFrame
    csv_file_path = rf"{TEST_DATA_DIR}/xcme/6EH4.XCME_1min_bars_20240101_20240131.csv.gz"
    df = pd.read_csv(csv_file_path, header=0, index_col=False)

    # Step 4b: Restructure DataFrame into required structure, that can be passed `BarDataWrangler`
    #  - 5 required columns: 'open', 'high', 'low', 'close', 'volume' (volume is optional)
    #  - column 'timestamp': should be in index of the DataFrame
    df = df.reindex(columns=["timestamp_utc", "open", "high", "low", "close", "volume"])
    df["timestamp_utc"] = pd.to_datetime(df["timestamp_utc"], format="%Y-%m-%d %H:%M:%S")
    df = df.rename(columns={"timestamp_utc": "timestamp"})
    df = df.set_index("timestamp")

    # Step 4c: Define type of loaded bars
    eurusd_1min_bartype = BarType.from_str(f"{eurusd_futures.id}-1-MINUTE-LAST-EXTERNAL")

    # Step 4d: `BarDataWrangler` converts each row into objects of type `Bar`
    wrangler = BarDataWrangler(eurusd_1min_bartype, eurusd_futures)
    eurusd_1min_bars_from_csv: list[Bar] = wrangler.process(df)

    # Step 4e: Add loaded data to the engine
    engine.add_data(eurusd_1min_bars_from_csv)

    # ----------------------------------------------------
    # Step 5: Data Catalog Management
    #
    # Data Catalog introduction:
    #   - A centralized repository for storing and managing trading data.
    #   - Provides functionalities to store, retrieve, and manage various types of market data efficiently.
    #   - Excellent for handling and compressing large datasets.
    # ----------------------------------------------------

    # Step 5a: Create a Data Catalog
    # (folder will be created if it doesn't exist)
    data_catalog = ParquetDataCatalog("./data_catalog")

    # Step 5b: Write Data to the Catalog
    data_catalog.write_data([eurusd_futures])  # Store instrument definition(s)
    data_catalog.write_data(eurusd_1min_bars_from_csv)  # Store bar data

    # Step 5c: Read Data from the Catalog

    # Retrieve all instrument definitions stored in the catalog (useful to check what's available)
    all_instruments = data_catalog.instruments()
    log.info(f"All instruments:\n{all_instruments}", color=LogColor.YELLOW)

    # Get all available bars (no filters = returns all bars for all instruments)
    all_bars = data_catalog.bars()
    log.info(f"All bars count: {len(all_bars)}", color=LogColor.YELLOW)

    # Get specific bars (filter by date range and instrument)
    filtered_bars = data_catalog.bars(
        bar_types=[f"{eurusd_futures.id}-1-MINUTE-LAST-EXTERNAL"],
        start="2024-01-10",  # optional filter
        end="2024-01-15",  # optional filter
    )
    log.info(f"Bars between Jan 10-15: {len(filtered_bars)}", color=LogColor.YELLOW)

    # List all types of data stored in the catalog
    # This helps understand what kind of data is available for retrieval
    data_types_in_catalog = data_catalog.list_data_types()
    log.info(f"Data types stored in catalog\n{data_types_in_catalog}", color=LogColor.YELLOW)

    # ----------------------------------------------------
    # Step 6: Create Strategy and Add it to the Engine
    # ----------------------------------------------------

    strategy = DemoStrategy(bar_type_1min=eurusd_1min_bartype)
    engine.add_strategy(strategy)

    # ----------------------------------------------------
    # Step 7: Run the Backtest Engine
    # ----------------------------------------------------

    engine.run()

    # ----------------------------------------------------
    # Step 8: Clean up resources
    # ----------------------------------------------------

    engine.dispose()
