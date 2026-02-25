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

from decimal import Decimal
from pathlib import Path

import pandas as pd

from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.bb_mean_reversion import BBMeanReversion
from nautilus_trader.examples.strategies.bb_mean_reversion import BBMeanReversionConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import PerpetualContract
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

if __name__ == "__main__":
    instrument_id = InstrumentId.from_str("EURUSD-PERP.AX")

    # Download free CSV data from https://www.truefx.com/truefx-historical-downloads/
    # The raw format has no headers: pair,timestamp,bid,ask
    data_path = Path("EURUSD-2025-12.csv")

    if not data_path.exists():
        raise FileNotFoundError(
            f"TrueFX data file not found: {data_path}\n"
            "Download free EUR/USD tick data from https://www.truefx.com/truefx-historical-downloads/\n"
            "and place the CSV file in the current directory.",
        )

    df = pd.read_csv(
        data_path,
        header=None,
        names=["pair", "timestamp", "bid", "ask"],
    )
    df["timestamp"] = pd.to_datetime(df["timestamp"], format="%Y%m%d %H:%M:%S.%f")
    df = df.set_index("timestamp")
    df = df[["bid", "ask"]]

    EURUSD_PERP = PerpetualContract(
        instrument_id=instrument_id,
        raw_symbol=Symbol("EURUSD-PERP"),
        underlying="EUR",
        asset_class=AssetClass.FX,
        quote_currency=USD,
        settlement_currency=USD,
        is_inverse=False,
        price_precision=5,
        size_precision=0,
        price_increment=Price.from_str("0.00001"),
        size_increment=Quantity.from_int(1),
        multiplier=Quantity.from_int(1000),
        lot_size=Quantity.from_int(1),
        margin_init=Decimal("0.05"),
        margin_maint=Decimal("0.025"),
        maker_fee=Decimal("0.0002"),
        taker_fee=Decimal("0.0005"),
        ts_event=0,
        ts_init=0,
    )

    wrangler = QuoteTickDataWrangler(instrument=EURUSD_PERP)
    ticks = wrangler.process(df)

    config = BacktestEngineConfig(
        trader_id=TraderId("BACKTESTER-001"),
        logging=LoggingConfig(log_level="INFO"),
    )

    engine = BacktestEngine(config=config)

    AX = Venue("AX")
    engine.add_venue(
        venue=AX,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(100_000, USD)],
    )

    engine.add_instrument(EURUSD_PERP)
    engine.add_data(ticks)

    bar_type = BarType.from_str("EURUSD-PERP.AX-1-MINUTE-MID-INTERNAL")

    strategy_config = BBMeanReversionConfig(
        instrument_id=instrument_id,
        bar_type=bar_type,
        trade_size=Decimal(1),
        bb_period=20,
        bb_std=2.0,
        rsi_period=14,
        rsi_buy_threshold=0.30,
        rsi_sell_threshold=0.70,
    )

    strategy = BBMeanReversion(config=strategy_config)
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
        print(engine.trader.generate_account_report(AX))
        print(engine.trader.generate_order_fills_report())
        print(engine.trader.generate_positions_report())

    engine.reset()
    engine.dispose()
