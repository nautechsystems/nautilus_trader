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
    This example demonstrates how to use NautilusTrader's bar aggregation features with
    the high-level BacktestNode API to create higher timeframe bars from lower timeframe
    data.

    The strategy shows how to:
    - Load 1-minute bar data from a data catalog.
    - Create 5-minute bars from 1-minute data using internal aggregation.
    - Handle both timeframes independently in the same strategy.

    This helps you understand how bar aggregation works in NautilusTrader and how
    to handle multiple timeframes in your strategies.

    """

    # ----------------------------------------------------------------------------------
    # 1. Prepare market data and create data catalog
    # ----------------------------------------------------------------------------------

    prepared_data: dict = prepare_demo_data_eurusd_futures_1min()
    venue_name: str = prepared_data["venue_name"]
    eurusd_instrument = prepared_data["instrument"]
    eurusd_1min_bartype = prepared_data["bar_type"]
    eurusd_1min_bars = prepared_data["bars_list"]

    # Create data catalog and write data to it
    catalog_path = "./data_catalog_example_03"
    data_catalog = ParquetDataCatalog(catalog_path)

    # Write instrument and bar data to catalog
    data_catalog.write_data([eurusd_instrument])  # Store instrument definition
    data_catalog.write_data(eurusd_1min_bars)  # Store bar data

    # ----------------------------------------------------------------------------------
    # 2. Configure BacktestNode with high-level API
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
    # 3. Create and run BacktestNode
    # ----------------------------------------------------------------------------------

    # Create the backtest node
    node = BacktestNode(configs=[run_config])

    # Execute the backtest
    results = node.run()

    # Clean up resources
    node.dispose()
