#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import ccxt
import pandas as pd

from examples.strategies.ema_cross_simple import EMACross
from nautilus_trader.adapters.ccxt.providers import CCXTInstrumentProvider
from nautilus_trader.backtest.data_container import BacktestDataContainer
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from tests.test_kit.providers import TestDataProvider


if __name__ == "__main__":
    # Setup trading instruments
    # Requires an internet connection for the instrument loader
    # Alternatively use the TestInstrumentProvider in the test kit
    print("Loading instruments...")
    instruments = CCXTInstrumentProvider(client=ccxt.binance(), load_all=True)

    BINANCE = Venue("BINANCE")
    ETHUSDT_BINANCE = instruments.get(Symbol("ETH/USDT", BINANCE))

    # Setup data container
    data = BacktestDataContainer()
    data.add_instrument(ETHUSDT_BINANCE)
    data.add_trade_ticks(ETHUSDT_BINANCE.symbol, TestDataProvider.ethusdt_trades())

    # Instantiate your strategy
    strategy = EMACross(
        symbol=ETHUSDT_BINANCE.symbol,
        bar_spec=BarSpecification(250, BarAggregation.TICK, PriceType.LAST),
        fast_ema=10,
        slow_ema=20,
        trade_size=Decimal(100),
    )

    # Build the backtest engine
    engine = BacktestEngine(
        data=data,
        strategies=[strategy],  # List of 'any' number of strategies
        # exec_db_type="redis",
    )

    # Create a fill model (optional)
    fill_model = FillModel(
        prob_fill_at_limit=0.2,
        prob_fill_at_stop=0.95,
        prob_slippage=0.5,
        random_seed=42,
    )

    # Add an exchange (now multiple exchanges possible)
    engine.add_exchange(
        venue=BINANCE,
        oms_type=OMSType.NETTING,
        generate_position_ids=False,
        starting_balances=[Money(1_000_000, USDT), Money(1, BTC)],  # now single-asset or multi-asset accounts
        fill_model=fill_model,
    )

    input("Press Enter to continue...")  # noqa (always Python 3)

    # Run the engine (from start to end of data)
    engine.run()

    # Optionally view reports
    with pd.option_context(
            "display.max_rows",
            100,
            "display.max_columns",
            None,
            "display.width", 300):
        print(engine.trader.generate_account_report(BINANCE))
        print(engine.trader.generate_order_fills_report())
        print(engine.trader.generate_positions_report())

    # Good practice to dispose of the object
    engine.dispose()
