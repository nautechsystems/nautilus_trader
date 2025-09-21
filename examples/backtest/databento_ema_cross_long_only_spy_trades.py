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

import time
from decimal import Decimal

import pandas as pd

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.databento import DatabentoDataLoader
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.ema_cross_long_only import EMACrossLongOnly
from nautilus_trader.examples.strategies.ema_cross_long_only import EMACrossLongOnlyConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.test_kit.providers import TestInstrumentProvider


if __name__ == "__main__":
    # Configure backtest engine
    config = BacktestEngineConfig(
        trader_id=TraderId("BACKTESTER-001"),
        logging=LoggingConfig(log_level="INFO"),
    )

    # Build the backtest engine
    engine = BacktestEngine(config=config)

    # Add a trading venue (multiple venues possible)
    NASDAQ = Venue("XNAS")
    engine.add_venue(
        venue=NASDAQ,
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        base_currency=USD,
        starting_balances=[Money(1_000_000.0, USD)],
    )

    # Add instruments
    SPY_XNAS = TestInstrumentProvider.equity(symbol="SPY", venue="XNAS")
    engine.add_instrument(SPY_XNAS)

    # Add data
    loader = DatabentoDataLoader()

    filenames = [
        "spy-xnas-trades-2024-01.dbn.zst",
        "spy-xnas-trades-2024-02.dbn.zst",
        "spy-xnas-trades-2024-03.dbn.zst",
    ]

    for filename in filenames:
        trades = loader.from_dbn_file(
            path=TEST_DATA_DIR / "databento" / "temp" / filename,
            instrument_id=SPY_XNAS.id,
        )
        engine.add_data(trades)

    # Configure your strategy
    strategy_config = EMACrossLongOnlyConfig(
        instrument_id=SPY_XNAS.id,
        bar_type=BarType.from_str(f"{SPY_XNAS.id}-1000-TICK-LAST-INTERNAL"),
        trade_size=Decimal(100),
        fast_ema_period=10,
        slow_ema_period=20,
        request_historical_bars=False,  # Using internally aggregated tick bars
    )

    # Instantiate and add your strategy
    strategy = EMACrossLongOnly(config=strategy_config)
    engine.add_strategy(strategy=strategy)

    time.sleep(0.1)
    input("Press Enter to continue...")

    # Run the engine (from start to end of data)
    engine.run()

    # Optionally view reports
    with pd.option_context(
        "display.max_rows",
        100,
        "display.max_columns",
        None,
        "display.width",
        300,
    ):
        print(engine.trader.generate_account_report(NASDAQ))
        print(engine.trader.generate_order_fills_report())
        print(engine.trader.generate_positions_report())

    # For repeated backtest runs make sure to reset the engine
    engine.reset()

    # Good practice to dispose of the object
    engine.dispose()
