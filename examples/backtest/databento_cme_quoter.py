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
from pathlib import Path

import pandas as pd

from nautilus_trader.adapters.databento import DatabentoDataLoader
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.examples.strategies.simpler_quoter import SimpleQuoterStrategy
from nautilus_trader.examples.strategies.simpler_quoter import SimpleQuoterStrategyConfig
from nautilus_trader.model.currencies import USD
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
        risk_engine=RiskEngineConfig(bypass=True),
    )

    # Build the backtest engine
    engine = BacktestEngine(config=config)

    # Add a trading venue (multiple venues possible)
    XCME = Venue("XCME")  # <-- ISO 10383 MIC
    engine.add_venue(
        venue=XCME,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000.0, USD)],
    )

    # Add instruments
    ESZ5 = TestInstrumentProvider.es_future(
        expiry_year=2025,
        expiry_month=12,
        venue=XCME,
    )
    engine.add_instrument(ESZ5)

    # Add data
    loader = DatabentoDataLoader()

    paths = [
        "~/Downloads/GLBX-20251023-C8KMULLDMW/glbx-mdp3-20251012.mbp-1.dbn.zst",
        # "/Downloads/GLBX-20251023-C8KMULLDMW/glbx-mdp3-20251013.mbp-1.dbn.zst",
    ]

    for path in paths:
        quotes = loader.from_dbn_file(
            path=Path(path).expanduser(),
            instrument_id=ESZ5.id,
        )
        engine.add_data(quotes)

    # Configure your strategy
    config_strategy = SimpleQuoterStrategyConfig(
        instrument_id=ESZ5.id,
        tob_offset_ticks=0,
        log_data=False,
    )

    # Instantiate and add your strategy
    strategy = SimpleQuoterStrategy(config=config_strategy)
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
        print(engine.trader.generate_account_report(XCME))
        print(engine.trader.generate_order_fills_report())
        print(engine.trader.generate_positions_report())

    # For repeated backtest runs make sure to reset the engine
    engine.reset()

    # Good practice to dispose of the object
    engine.dispose()
