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

from datetime import UTC
from datetime import datetime
from decimal import Decimal

from strategy import DemoStrategy

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import TraderId
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity
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
    venue_name = "XCME"
    XCME = Venue(venue_name)
    engine.add_venue(
        venue=XCME,
        oms_type=OmsType.NETTING,  # Order Management System type
        account_type=AccountType.MARGIN,  # Type of trading account
        starting_balances=[Money(1_000_000, USD)],  # Initial account balance
        base_currency=USD,  # Base currency for account
        default_leverage=Decimal(1),  # No leverage used for account
    )

    # Step 3: Create instrument definition and add it to the engine
    instrument = TestInstrumentProvider.eurusd_future(
        expiry_year=2024,
        expiry_month=3,
        venue_name=venue_name,
    )
    engine.add_instrument(instrument)

    # -------------------------------------------------------
    # PREPARE DATA
    # -------------------------------------------------------

    # Step 4a: Prepare BarType
    bar_type = BarType.from_str(f"{instrument.id}-1-MINUTE-LAST-EXTERNAL")

    # Step 4b: Prepare bar data in form of List[Bar]
    # Let's generate artificial 1-min bars
    count_of_generated_bars = 10
    start_datetime = dt_to_unix_nanos(datetime(2024, 2, 1, tzinfo=UTC))
    bars = []
    for i in range(count_of_generated_bars):
        price_offset = i * float(instrument.price_increment)
        bar = Bar(
            bar_type=bar_type,
            open=instrument.make_price(1.10000 + price_offset),
            high=instrument.make_price(1.20000 + price_offset),
            low=instrument.make_price(1.10000 + price_offset),
            close=instrument.make_price(1.10000 + price_offset),
            volume=Quantity.from_int(999999),
            ts_event=start_datetime + (i * 60 * 1_000_000_000),  # +1 minute (in nanoseconds)
            ts_init=start_datetime + (i * 60 * 1_000_000_000),
        )
        bars.append(bar)

    # Step 4c: Add loaded data to the engine
    engine.add_data(bars)

    # ------------------------------------------------------------------------------------------

    # Step 5: Create strategy and add it to the engine
    strategy = DemoStrategy(input_bartype=bar_type)
    engine.add_strategy(strategy)

    # Step 6: Run engine = Run backtest
    engine.run()

    # Step 7: Release system resources
    engine.dispose()
