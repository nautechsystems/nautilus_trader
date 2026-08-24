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

import asyncio
import sys
from decimal import Decimal
from pathlib import Path

import pandas as pd

from nautilus_trader.adapters.binance import BINANCE_VENUE
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceInstrumentProviderConfig
from nautilus_trader.adapters.binance import BinanceProductType
from nautilus_trader.adapters.binance import load_binance_instruments
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import RiskEngineConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import BarType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import TraderId
from nautilus_trader.testkit.providers import TestDataProvider


sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "docs" / "tutorials"))

from ema_cross import EMACross
from ema_cross import EMACrossConfig


async def load_instrument(instrument_id: InstrumentId):
    instruments = await load_binance_instruments(
        BinanceDataClientConfig(
            product_type=BinanceProductType.USD_M,
            instrument_provider=BinanceInstrumentProviderConfig(
                load_all=False,
                load_ids=[str(instrument_id)],
                log_warnings=False,
            ),
        ),
    )

    if len(instruments) != 1:
        raise RuntimeError(f"Expected one Binance instrument for {instrument_id}")
    return instruments[0]


if __name__ == "__main__":
    instrument_id = InstrumentId.from_str("BTCUSDT-PERP.BINANCE")
    instrument = asyncio.run(load_instrument(instrument_id))

    engine = BacktestEngine(
        BacktestEngineConfig(
            trader_id=TraderId.from_str("BACKTESTER-001"),
            risk_engine=RiskEngineConfig(bypass=True),
        ),
    )
    engine.add_venue(
        venue=BINANCE_VENUE,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=None,
        starting_balances=[Money(1_000_000, instrument.quote_currency)],
    )
    engine.add_instrument(instrument)

    bar_type = BarType.from_str("BTCUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL")
    bars = TestDataProvider.bars_from_binance_csv(
        instrument=instrument,
        bar_type=bar_type,
        csv_name="btc-perp-20211231-20220201_1m.csv",
    )
    engine.add_data(bars)

    strategy = EMACross(
        EMACrossConfig(
            instrument_id=instrument.id,
            bar_type=bar_type,
            trade_size=Decimal("0.010"),
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
        print(engine.generate_account_report(BINANCE_VENUE))
        print(engine.generate_order_fills_report())
        print(engine.generate_positions_report())

    engine.reset()
    engine.dispose()
