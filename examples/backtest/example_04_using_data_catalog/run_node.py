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

from strategy import DemoStrategy
from strategy import DemoStrategyConfig

from examples.utils.data_provider import prepare_demo_data_eurusd_futures_1min
from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.common.config import LoggingConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.model.data import Bar
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog


if __name__ == "__main__":
    """
    This example demonstrates how to use a data catalog with the high-level BacktestNode
    API.

    The example shows how to:
    - Create a data catalog and store market data
    - Read and analyze data from the catalog
    - Use the catalog as a data source for backtesting with BacktestNode

    This approach provides better data management, persistence, and reusability
    compared to loading data directly into the engine.

    """

    # ----------------------------------------------------------------------------------
    # 1. Prepare market data and create data catalog
    # ----------------------------------------------------------------------------------

    prepared_data: dict = prepare_demo_data_eurusd_futures_1min()
    venue_name: str = prepared_data["venue_name"]
    eurusd_instrument = prepared_data["instrument"]
    eurusd_1min_bartype = prepared_data["bar_type"]
    eurusd_1min_bars_list = prepared_data["bars_list"]

    # ----------------------------------------------------------------------------------
    # 2. Configure and use Data Catalog
    # ----------------------------------------------------------------------------------

    # Create a Data Catalog (folder will be created if it doesn't exist)
    catalog_path = "./data_catalog_example_04"
    data_catalog = ParquetDataCatalog(catalog_path)

    # Write data to the catalog
    data_catalog.write_data([eurusd_instrument])  # Store instrument definition(s)
    data_catalog.write_data(eurusd_1min_bars_list)  # Store bar data

    # Read and analyze data from the catalog (optional - for demonstration)
    # - Retrieve all instrument definitions
    all_instruments = data_catalog.instruments()
    print(f"All instruments: {len(all_instruments)}")

    # - Get all available bars
    all_bars = data_catalog.bars()
    print(f"All bars count: {len(all_bars)}")

    # - Get specific bars with date range filter
    filtered_bars = data_catalog.bars(
        bar_types=[str(eurusd_1min_bartype)],
        start="2024-01-10",  # Filter start date
        end="2024-01-15",  # Filter end date
    )
    print(f"Bars between Jan 10-15: {len(filtered_bars)}")

    # - List all available data types
    data_types_in_catalog = data_catalog.list_data_types()
    print(f"Data types stored in catalog: {data_types_in_catalog}")

    # ----------------------------------------------------------------------------------
    # 3. Configure BacktestNode with high-level API
    # ----------------------------------------------------------------------------------

    # Configure strategy using ImportableStrategyConfig
    strategies = [
        ImportableStrategyConfig(
            strategy_path=DemoStrategy.fully_qualified_name(),
            config_path=DemoStrategyConfig.fully_qualified_name(),
            config={
                "bar_type_1min": str(eurusd_1min_bartype),
            },
        ),
    ]

    # Configure logging
    logging = LoggingConfig(
        log_level="DEBUG",  # Set to DEBUG to see detailed timer and bar processing logs
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
            name=venue_name,
            oms_type="NETTING",  # Netting: positions are netted against each other
            account_type="MARGIN",  # Margin account: allows trading with leverage
            starting_balances=["1_000_000 USD"],  # Initial account balance of $1,000,000 USD
            base_currency="USD",  # Account base currency is USD
            default_leverage=1.0,  # No leverage is used (1:1)
        ),
    ]

    # Configure data source from catalog
    data = [
        BacktestDataConfig(
            catalog_path=catalog_path,
            data_cls=Bar,
            instrument_id=eurusd_instrument.id,
        ),
    ]

    # Create BacktestRunConfig
    run_config = BacktestRunConfig(
        engine=engine_config,
        venues=venues,
        data=data,
    )

    # ----------------------------------------------------------------------------------
    # 4. Create and run BacktestNode
    # ----------------------------------------------------------------------------------

    # Create the backtest node
    node = BacktestNode(configs=[run_config])

    # Execute the backtest
    results = node.run()

    # Clean up resources
    node.dispose()
