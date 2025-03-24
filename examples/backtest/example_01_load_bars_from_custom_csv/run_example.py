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
            log_level="DEBUG",  # set DEBUG log level for console to see loaded bars in logs
        ),
    )
    engine = BacktestEngine(config=engine_config)

    # Step 2: Define exchange and add it to the engine
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
    EURUSD_FUTURES_INSTRUMENT = TestInstrumentProvider.eurusd_future(
        expiry_year=2024,
        expiry_month=3,
        venue_name="XCME",
    )
    engine.add_instrument(EURUSD_FUTURES_INSTRUMENT)

    # ==========================================================================================
    # POINT OF FOCUS: Loading bars from CSV
    # ------------------------------------------------------------------------------------------

    # Step 4a: Load bar data from CSV file -> into pandas DataFrame
    csv_file_path = r"6EH4.XCME_1min_bars.csv"
    df = pd.read_csv(csv_file_path, sep=";", decimal=".", header=0, index_col=False)

    # Step 4b: Restructure DataFrame into required structure, that can be passed `BarDataWrangler`
    #   - 5 columns: 'open', 'high', 'low', 'close', 'volume' (volume is optional)
    #   - 'timestamp' as index

    # Change order of columns
    df = df.reindex(columns=["timestamp_utc", "open", "high", "low", "close", "volume"])
    # Convert string timestamps into datetime
    df["timestamp_utc"] = pd.to_datetime(df["timestamp_utc"], format="%Y-%m-%d %H:%M:%S")
    # Rename column to required name
    df = df.rename(columns={"timestamp_utc": "timestamp"})
    # Seet column `timestamp` as index
    df = df.set_index("timestamp")

    # Step 4c: Define type of loaded bars
    EURUSD_FUTURES_1MIN_BARTYPE = BarType.from_str(
        f"{EURUSD_FUTURES_INSTRUMENT.id}-1-MINUTE-LAST-EXTERNAL",
    )

    # Step 4d: `BarDataWrangler` converts each row into objects of type `Bar`
    wrangler = BarDataWrangler(EURUSD_FUTURES_1MIN_BARTYPE, EURUSD_FUTURES_INSTRUMENT)
    eurusd_1min_bars_list: list[Bar] = wrangler.process(df)

    # Step 4e: Add loaded data to the engine
    engine.add_data(eurusd_1min_bars_list)

    # ------------------------------------------------------------------------------------------
    # END OF POINT OF FOCUS
    # ==========================================================================================

    # Step 5: Create strategy and add it to the engine
    strategy = DemoStrategy(primary_bar_type=EURUSD_FUTURES_1MIN_BARTYPE)
    engine.add_strategy(strategy)

    # Step 6: Run engine = Run backtest
    engine.run()

    # Step 7: Release system resources
    engine.dispose()
