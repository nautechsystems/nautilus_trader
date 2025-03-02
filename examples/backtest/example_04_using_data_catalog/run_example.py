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

from strategy import DemoStrategy

from examples.utils.data_provider import prepare_demo_data_eurusd_futures_1min
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.core.nautilus_pyo3 import BarType
from nautilus_trader.model import Bar
from nautilus_trader.model import TraderId
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog


log = Logger(__name__)


if __name__ == "__main__":

    # ----------------------------------------------------------------------------------
    # Step 1: Configure and Create the Backtest Engine
    # ----------------------------------------------------------------------------------

    engine_config = BacktestEngineConfig(
        trader_id=TraderId("BACKTEST_TRADER-001"),
        logging=LoggingConfig(
            log_level="DEBUG",  # Set to DEBUG to see detailed timer and bar processing logs
        ),
    )
    engine = BacktestEngine(config=engine_config)

    # ----------------------------------------------------------------------------------
    # 2. Prepare market data
    # ----------------------------------------------------------------------------------

    prepared_data: dict = prepare_demo_data_eurusd_futures_1min()
    venue_name: str = prepared_data["venue_name"]
    eurusd_instrument: Instrument = prepared_data["instrument"]
    eurusd_1min_bartype: BarType = prepared_data["bar_type"]
    eurusd_1min_bars_list: list[Bar] = prepared_data["bars_list"]

    # ----------------------------------------------------------------------------------
    # 3. Configure trading environment
    # ----------------------------------------------------------------------------------

    # Set up the trading venue with a margin account
    engine.add_venue(
        venue=Venue(venue_name),
        oms_type=OmsType.NETTING,  # Netting: positions are netted against each other
        account_type=AccountType.MARGIN,  # Margin account: allows trading with leverage
        starting_balances=[Money(1_000_000, USD)],  # Initial account balance of $1,000,000 USD
        base_currency=USD,  # Account base currency is USD
        default_leverage=Decimal(1),  # No leverage is used (1:1)
    )

    # Add instrument and market data to the engine
    engine.add_instrument(eurusd_instrument)
    engine.add_data(eurusd_1min_bars_list)

    # ----------------------------------------------------------------------------------
    # 4. Configure and use Data Catalog
    # ----------------------------------------------------------------------------------

    # Create a Data Catalog (folder will be created if it doesn't exist)
    data_catalog = ParquetDataCatalog("./data_catalog")

    # Write data to the catalog
    data_catalog.write_data([eurusd_instrument])  # Store instrument definition(s)
    data_catalog.write_data(eurusd_1min_bars_list)  # Store bar data

    # Read and analyze data from the catalog
    # - Retrieve all instrument definitions
    all_instruments = data_catalog.instruments()
    log.info(f"All instruments:\n{all_instruments}", color=LogColor.YELLOW)

    # - Get all available bars
    all_bars = data_catalog.bars()
    log.info(f"All bars count: {len(all_bars)}", color=LogColor.YELLOW)

    # - Get specific bars with date range filter
    filtered_bars = data_catalog.bars(
        bar_types=[str(eurusd_1min_bartype)],
        start="2024-01-10",  # Filter start date
        end="2024-01-15",  # Filter end date
    )
    log.info(f"Bars between Jan 10-15: {len(filtered_bars)}", color=LogColor.YELLOW)

    # - List all available data types
    data_types_in_catalog = data_catalog.list_data_types()
    log.info(f"Data types stored in catalog\n{data_types_in_catalog}", color=LogColor.YELLOW)

    # ----------------------------------------------------------------------------------
    # 5. Configure and run strategy
    # ----------------------------------------------------------------------------------

    # Create and register the strategy
    strategy = DemoStrategy(bar_type_1min=eurusd_1min_bartype)
    engine.add_strategy(strategy)

    # Execute the backtest
    engine.run()

    # Clean up resources
    engine.dispose()
