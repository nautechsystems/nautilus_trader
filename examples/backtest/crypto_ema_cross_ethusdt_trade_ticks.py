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
import os
import sys

import ccxt
import pandas as pd


sys.path.insert(
    0, str(os.path.abspath(__file__ + "/../../../"))
)  # Allows relative imports from examples

from examples.strategies.ema_cross_simple import EMACross
from nautilus_trader.adapters.ccxt.providers import CCXTInstrumentProvider
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import InstrumentId
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

    # Build the backtest engine
    engine = BacktestEngine(
        use_data_cache=True,  # Pre-cache data for increased performance on repeated runs
        # cache_db_type="redis",
        # bypass_logging=True
    )

    BINANCE = Venue("BINANCE")
    instrument_id = InstrumentId(symbol=Symbol("ETH/USDT"), venue=BINANCE)
    ETHUSDT_BINANCE = instruments.find(instrument_id)

    # Setup data
    engine.add_instrument(ETHUSDT_BINANCE)
    engine.add_trade_ticks(ETHUSDT_BINANCE.id, TestDataProvider.ethusdt_trades())

    # Create a fill model (optional)
    fill_model = FillModel(
        prob_fill_at_limit=0.2,
        prob_fill_at_stop=0.95,
        prob_slippage=0.5,
        random_seed=42,
    )

    # Add an exchange (multiple exchanges possible)
    # Add starting balances for single-currency or multi-currency accounts
    engine.add_venue(
        venue=BINANCE,
        venue_type=VenueType.EXCHANGE,
        oms_type=OMSType.NETTING,
        account_type=AccountType.CASH,  # Spot cash account
        base_currency=None,  # Multi-currency account
        starting_balances=[Money(1_000_000, USDT), Money(10, ETH)],
        fill_model=fill_model,
    )

    # Instantiate your strategy
    strategy = EMACross(
        instrument_id=ETHUSDT_BINANCE.id,
        bar_spec=BarSpecification(250, BarAggregation.TICK, PriceType.LAST),
        fast_ema_period=10,
        slow_ema_period=20,
        trade_size=Decimal("0.05"),
        order_id_tag="001",
    )

    input("Press Enter to continue...")  # noqa (always Python 3)

    # Run the engine (from start to end of data)
    engine.run(strategies=[strategy])

    # Optionally view reports
    with pd.option_context(
        "display.max_rows",
        100,
        "display.max_columns",
        None,
        "display.width",
        300,
    ):
        print(engine.trader.generate_account_report(BINANCE))
        print(engine.trader.generate_order_fills_report())
        print(engine.trader.generate_positions_report())

    # For repeated backtest runs make sure to reset the engine
    engine.reset()

    # Good practice to dispose of the object
    engine.dispose()
