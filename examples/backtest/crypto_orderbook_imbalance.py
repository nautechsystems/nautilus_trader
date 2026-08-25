#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import sys
from pathlib import Path

import pandas as pd

from nautilus_trader.adapters.binance import BINANCE_VENUE
from nautilus_trader.adapters.binance import load_binance_order_book_deltas
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import TraderId
from nautilus_trader.testkit.providers import TestInstrumentProvider


sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "docs" / "tutorials"))

from orderbook_data import deltas_from_frame
from orderbook_imbalance import OrderBookImbalance
from orderbook_imbalance import OrderBookImbalanceConfig


if __name__ == "__main__":
    engine = BacktestEngine(
        BacktestEngineConfig(trader_id=TraderId.from_str("BACKTESTER-001")),
    )
    BTC = Currency.from_str("BTC")
    USDT = Currency.from_str("USDT")
    engine.add_venue(
        venue=BINANCE_VENUE,
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        base_currency=None,
        starting_balances=[Money(100, BTC), Money(1_000_000, USDT)],
        book_type=BookType.L2_MBP,
    )

    BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
    engine.add_instrument(BTCUSDT_BINANCE)

    data_dir = Path(__file__).resolve().parents[2] / "test_data" / "binance"
    snapshot = load_binance_order_book_deltas(data_dir / "btcusdt-depth-snap.csv")
    updates = load_binance_order_book_deltas(data_dir / "btcusdt-depth-update.csv")
    deltas = deltas_from_frame(snapshot, BTCUSDT_BINANCE)
    deltas += deltas_from_frame(updates, BTCUSDT_BINANCE)
    deltas.sort(key=lambda delta: delta.ts_init)
    engine.add_data(deltas)

    strategy = OrderBookImbalance(
        OrderBookImbalanceConfig(
            instrument_id=str(BTCUSDT_BINANCE.id),
            max_trade_size="1.000",
            trigger_min_size=20.0,
            trigger_imbalance_ratio=0.20,
            min_seconds_between_triggers=1.0,
            book_type="L2_MBP",
        ),
    )
    engine.add_strategy(strategy)
    engine.run()

    with pd.option_context(
        "display.max_rows",
        100,
        "display.max_columns",
        None,
        "display.width",
        300,
    ):
        print(engine.generate_account_report(BINANCE_VENUE))
        print(engine.generate_order_fills_report())
        print(engine.generate_positions_report())

    engine.reset()
    engine.dispose()
