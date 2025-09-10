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
from pathlib import Path

import pandas as pd

from nautilus_trader.adapters.binance import BINANCE_VENUE
from nautilus_trader.adapters.binance.loaders import BinanceOrderBookDeltaDataLoader
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import book_type_to_str
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import OrderBookDeltaDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider


if __name__ == "__main__":
    # Configure backtest engine
    config = BacktestEngineConfig(
        trader_id=TraderId("BACKTESTER-001"),
        # logging=LoggingConfig(log_level="DEBUG"),
    )

    # Build the backtest engine
    engine = BacktestEngine(config=config)

    # Add a trading venue (multiple venues possible)
    # Ensure the book type matches the data
    book_type = BookType.L2_MBP

    engine.add_venue(
        venue=BINANCE_VENUE,
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        base_currency=None,  # Multi-currency account
        starting_balances=[Money(1_000_000.0, USDT), Money(100.0, BTC)],
        book_type=book_type,  # <-- Venues order book
    )

    # Add instruments
    BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
    engine.add_instrument(BTCUSDT_BINANCE)

    # Add data
    data_dir = Path("~/Downloads").expanduser() / "Data" / "Binance"

    path_snap = data_dir / "BTCUSDT_T_DEPTH_2022-11-01_depth_snap.csv"
    print(f"Loading {path_snap} ...")
    df_snap = BinanceOrderBookDeltaDataLoader.load(path_snap)
    print(str(df_snap))

    path_update = data_dir / "BTCUSDT_T_DEPTH_2022-11-01_depth_update.csv"
    print(f"Loading {path_update} ...")
    nrows = 1_000_000
    df_update = BinanceOrderBookDeltaDataLoader.load(path_update, nrows=nrows)
    print(str(df_update))

    print("Wrangling OrderBookDelta objects ...")
    wrangler = OrderBookDeltaDataWrangler(instrument=BTCUSDT_BINANCE)
    deltas = wrangler.process(df_snap)
    deltas += wrangler.process(df_update)
    engine.add_data(deltas)

    # Configure your strategy
    strategy_config = OrderBookImbalanceConfig(
        instrument_id=BTCUSDT_BINANCE.id,
        max_trade_size=Decimal("1.000"),
        min_seconds_between_triggers=1.0,
        book_type=book_type_to_str(book_type),
    )

    # Instantiate and add your strategy
    strategy = OrderBookImbalance(config=strategy_config)
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
        print(engine.trader.generate_account_report(BINANCE_VENUE))
        print(engine.trader.generate_order_fills_report())
        print(engine.trader.generate_positions_report())

    # For repeated backtest runs make sure to reset the engine
    engine.reset()

    # Good practice to dispose of the object
    engine.dispose()
