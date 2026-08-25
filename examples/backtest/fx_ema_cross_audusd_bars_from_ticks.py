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
Example of fx ema cross audusd bars from ticks.
"""

import sys
from decimal import Decimal
from pathlib import Path

import pandas as pd

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import FXRolloverInterestModule
from nautilus_trader.backtest import InterestRateRecord
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import BarType
from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.testkit.providers import TestDataProvider
from nautilus_trader.testkit.providers import TestInstrumentProvider


sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "docs" / "tutorials"))

from ema_cross import EMACross
from ema_cross import EMACrossConfig


if __name__ == "__main__":
    provider = TestDataProvider()
    rates = provider.read_csv("short-term-interest.csv")
    rollover = FXRolloverInterestModule(
        records=[
            InterestRateRecord(location=row.LOCATION, time=row.TIME, value=row.Value)
            for row in rates.itertuples(index=False)
        ],
    )

    engine = BacktestEngine(
        BacktestEngineConfig(trader_id=TraderId.from_str("BACKTESTER-001")),
    )
    SIM = Venue("SIM")
    USD = Currency.from_str("USD")
    engine.add_venue(
        venue=SIM,
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
        modules=[rollover],
    )

    AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD", SIM)
    engine.add_instrument(AUDUSD_SIM)

    ticks = provider.quotes_from_truefx_csv(
        instrument=AUDUSD_SIM,
        csv_name="truefx/audusd-ticks.csv",
    )
    engine.add_data(ticks)

    strategy = EMACross(
        EMACrossConfig(
            instrument_id=AUDUSD_SIM.id,
            bar_type=BarType.from_str("AUD/USD.SIM-1-MINUTE-MID-INTERNAL"),
            trade_size=Decimal(1_000_000),
            fast_ema_period=10,
            slow_ema_period=20,
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
        print(engine.generate_account_report(SIM))
        print(engine.generate_order_fills_report())
        print(engine.generate_positions_report())

    engine.reset()
    engine.dispose()
