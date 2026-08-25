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
"""
Example of fx market maker gbpusd bars.
"""

import pandas as pd

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.execution import ProbabilisticFillModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import Quantity
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.testkit.providers import TestDataProvider
from nautilus_trader.testkit.providers import TestInstrumentProvider
from nautilus_trader.trading import GridMarketMakerConfig


if __name__ == "__main__":
    engine = BacktestEngine(
        BacktestEngineConfig(trader_id=TraderId.from_str("BACKTESTER-001")),
    )
    SIM = Venue("SIM")
    USD = Currency.from_str("USD")
    engine.add_venue(
        venue=SIM,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(10_000_000, USD)],
        fill_model=ProbabilisticFillModel(
            prob_fill_on_limit=0.2,
            prob_slippage=0.5,
            random_seed=42,
        ),
    )

    GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD", SIM)
    engine.add_instrument(GBPUSD_SIM)

    quotes = TestDataProvider.quotes_from_fxcm_bars(
        instrument=GBPUSD_SIM,
        bid_csv="fxcm/gbpusd-m1-bid-2012.csv",
        ask_csv="fxcm/gbpusd-m1-ask-2012.csv",
        max_rows=10_000,
    )
    engine.add_data(quotes)

    engine.add_builtin_strategy(
        "GridMarketMaker",
        GridMarketMakerConfig(
            instrument_id=GBPUSD_SIM.id,
            max_position=Quantity.from_int(1_500_000),
            trade_size=Quantity.from_int(500_000),
            num_levels=3,
            grid_step_bps=5,
            skew_factor=0.5,
            requote_threshold_bps=2,
        ),
    )
    engine.run()

    with pd.option_context(
        "display.max_rows",
        100,
        "display.max_columns",
        None,
        "display.width",
        300,
    ):
        print(engine.generate_account_report(SIM))
        print(engine.generate_order_fills_report())
        print(engine.generate_positions_report())

    engine.reset()
    engine.dispose()
