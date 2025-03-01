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

from strategy import SimpleTimerStrategy

from examples.utils.data_provider import prepare_demo_data_eurusd_futures_1min
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model import TraderId
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money


if __name__ == "__main__":
    # Configure and create backtest engine
    engine_config = BacktestEngineConfig(
        trader_id=TraderId("BACKTEST_TRADER-001"),
        logging=LoggingConfig(
            log_level="DEBUG",  # set DEBUG log level for console to see loaded bars in logs
        ),
    )
    engine = BacktestEngine(config=engine_config)

    # REUSABLE DATA
    # This code is often the same in all examples, so it is moved to a separate reusable function
    prepared_data = prepare_demo_data_eurusd_futures_1min()
    VENUE_NAME = prepared_data["venue_name"]  # Exchange name
    EURUSD_INSTRUMENT = prepared_data["instrument"]  # Instrument object
    EURUSD_1MIN_BARTYPE = prepared_data["bar_type"]  # BarType object
    eurusd_1min_bars_list = prepared_data["bars_list"]  # List of Bar objects

    # Define exchange and add it to the engine
    engine.add_venue(
        venue=Venue(VENUE_NAME),
        oms_type=OmsType.NETTING,  # Order Management System type
        account_type=AccountType.MARGIN,  # Type of trading account
        starting_balances=[Money(1_000_000, USD)],  # Initial account balance
        base_currency=USD,  # Base currency for account
        default_leverage=Decimal(1),  # No leverage used for account
    )

    # Add instrument to the engine
    engine.add_instrument(EURUSD_INSTRUMENT)

    # Add bars to the engine
    engine.add_data(eurusd_1min_bars_list)

    # Create strategy and add it to the engine
    strategy = SimpleTimerStrategy(primary_bar_type=EURUSD_1MIN_BARTYPE)
    engine.add_strategy(strategy)

    # Run engine = Run backtest
    engine.run()

    # Release system resources
    engine.dispose()
