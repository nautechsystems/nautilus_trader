#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.algorithms.twap import TWAPExecAlgorithm
from nautilus_trader.examples.strategies.ema_cross_twap import EMACrossTWAP
from nautilus_trader.examples.strategies.ema_cross_twap import EMACrossTWAPConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from tests import TEST_DATA_DIR


if __name__ == "__main__":
    # Configure backtest engine
    config = BacktestEngineConfig(
        trader_id=TraderId("BACKTESTER-001"),
        logging=LoggingConfig(
            log_level="INFO",
            log_colors=True,
        ),
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
        starting_balances=[Money(10_000_000.0, USD)],
    )

    # Add instruments
    TSLA_NASDAQ = TestInstrumentProvider.equity(symbol="TSLA")
    engine.add_instrument(TSLA_NASDAQ)

    # Add data
    loader = DatabentoDataLoader()
    trades = loader.from_dbn_file(
        path=TEST_DATA_DIR / "databento" / "temp" / "tsla-xnas-20240107-20240206.trades.dbn.zst",
        instrument_id=TSLA_NASDAQ.id,
    )
    engine.add_data(trades)

    # Configure your strategy
    config = EMACrossTWAPConfig(
        instrument_id=TSLA_NASDAQ.id,
        bar_type=BarType.from_str("TSLA.XNAS-5-MINUTE-LAST-INTERNAL"),
        trade_size=Decimal(100),
        fast_ema_period=10,
        slow_ema_period=20,
        twap_horizon_secs=10.0,
        twap_interval_secs=2.5,
    )

    # Instantiate and add your strategy
    strategy = EMACrossTWAP(config=config)
    engine.add_strategy(strategy=strategy)

    # Instantiate and add your execution algorithm
    exec_algorithm = TWAPExecAlgorithm()
    engine.add_exec_algorithm(exec_algorithm)

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
