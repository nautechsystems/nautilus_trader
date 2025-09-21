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

from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import LoggingConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import TraderId
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider


NANOSECONDS_IN_SECOND = 1_000_000_000


def generate_artificial_bars(instrument: Instrument, bar_type: BarType) -> list[Bar]:
    # Changes between generated bars
    PRICE_CHANGE = instrument.price_increment.as_double() * 10  # 10 ticks
    TIME_CHANGE_NANOS = 60 * NANOSECONDS_IN_SECOND  # 1 minute

    # --------------------------------------------
    # CREATE 1ST BAR
    # --------------------------------------------

    first_bar_time_as_unix_nanos = dt_to_unix_nanos(
        datetime(2024, 2, 1, hour=0, minute=1, second=0, tzinfo=UTC),
    )

    # Add 1st bar
    first_bar = Bar(
        bar_type=bar_type,
        open=instrument.make_price(1.10250),
        high=instrument.make_price(1.10300),
        low=instrument.make_price(1.100000),
        close=instrument.make_price(1.10050),
        volume=Quantity.from_int(999999),
        ts_event=first_bar_time_as_unix_nanos + TIME_CHANGE_NANOS,
        ts_init=first_bar_time_as_unix_nanos + TIME_CHANGE_NANOS,
    )

    generated_bars = [first_bar]
    last_bar = generated_bars[-1]

    # --------------------------------------------
    # CREATE ADDITIONAL BARS
    # --------------------------------------------

    # Add some INCREASING bars
    for i in range(10):
        last_bar = Bar(
            bar_type=first_bar.bar_type,
            open=instrument.make_price(first_bar.open + PRICE_CHANGE),
            high=instrument.make_price(first_bar.high + PRICE_CHANGE),
            low=instrument.make_price(first_bar.low + PRICE_CHANGE),
            close=instrument.make_price(first_bar.close + PRICE_CHANGE),
            volume=first_bar.volume,
            ts_event=first_bar.ts_event + TIME_CHANGE_NANOS,
            ts_init=first_bar.ts_init + TIME_CHANGE_NANOS,
        )
        generated_bars.append(last_bar)

    # Add some DECREASING bars
    for i in range(10):
        last_bar = Bar(
            bar_type=first_bar.bar_type,
            open=instrument.make_price(first_bar.open - PRICE_CHANGE),
            high=instrument.make_price(first_bar.high - PRICE_CHANGE),
            low=instrument.make_price(first_bar.low - PRICE_CHANGE),
            close=instrument.make_price(first_bar.close - PRICE_CHANGE),
            volume=first_bar.volume,
            ts_event=first_bar.ts_event + TIME_CHANGE_NANOS,
            ts_init=first_bar.ts_init + TIME_CHANGE_NANOS,
        )
        generated_bars.append(last_bar)

    return generated_bars


def run_backtest():
    # Step 1: Configure and create backtest engine
    engine_config = BacktestEngineConfig(
        trader_id=TraderId("BACKTEST_TRADER-001"),
        # Configure how data will be processed
        data_engine=DataEngineConfig(
            time_bars_interval_type="left-open",
            time_bars_timestamp_on_close=True,
            time_bars_skip_first_non_full_bar=False,
            time_bars_build_with_no_updates=False,  # don't emit aggregated bars, when no source data
            validate_data_sequence=True,
        ),
        # Configure logging
        logging=LoggingConfig(
            log_level="DEBUG",  # set DEBUG log level for console to see loaded bars in logs
        ),
    )
    engine = BacktestEngine(config=engine_config)

    # Step 2: Define exchange and add it to the engine
    VENUE_NAME = "XCME"
    engine.add_venue(
        venue=Venue(VENUE_NAME),
        oms_type=OmsType.NETTING,  # Order Management System type
        account_type=AccountType.MARGIN,  # Type of trading account
        starting_balances=[Money(1_000_000, USD)],  # Initial account balance
        base_currency=USD,  # Base currency for account
        default_leverage=Decimal(1),  # No leverage used for account
    )

    # Step 3: Create instrument definition and add it to the engine
    EURUSD_FUTURE = TestInstrumentProvider.eurusd_future(
        expiry_year=2024,
        expiry_month=3,
        venue_name=VENUE_NAME,
    )
    engine.add_instrument(EURUSD_FUTURE)

    # -------------------------------------------------------
    # PREPARE DATA
    # -------------------------------------------------------

    # Step 4a: Prepare BarType
    EURUSD_1MIN_BARTYPE = BarType.from_str(f"{EURUSD_FUTURE.id}-1-MINUTE-LAST-EXTERNAL")

    # Step 4b: Prepare bar data as List[Bar]
    bars: list[Bar] = generate_artificial_bars(
        instrument=EURUSD_FUTURE,
        bar_type=EURUSD_1MIN_BARTYPE,
    )

    # Step 4c: Add loaded data to the engine
    engine.add_data(bars)

    # ------------------------------------------------------------------------------------------

    # Step 5: Create strategy and add it to the engine
    strategy = DemoStrategy(input_bartype=EURUSD_1MIN_BARTYPE)
    engine.add_strategy(strategy)

    # Step 6: Run engine = Run backtest
    engine.run()

    # Step 7: Release system resources
    engine.dispose()


if __name__ == "__main__":
    run_backtest()
