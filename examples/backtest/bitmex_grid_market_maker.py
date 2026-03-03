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

# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***
"""
Backtest using free Tardis quote data for XBTUSD on BitMEX.

Download the free first-of-month dataset before running:

    curl -LO https://datasets.tardis.dev/v1/bitmex/quotes/2024/01/01/XBTUSD.csv.gz

Then run:

    python examples/backtest/bitmex_grid_market_maker.py

"""

from decimal import Decimal
from pathlib import Path

import pandas as pd

from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.grid_market_maker import GridMarketMaker
from nautilus_trader.examples.strategies.grid_market_maker import GridMarketMakerConfig
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
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


if __name__ == "__main__":
    instrument_id = InstrumentId.from_str("XBTUSD.BITMEX")

    # Free first-of-month data, no API key required
    # https://datasets.tardis.dev/v1/bitmex/quotes/2024/01/01/XBTUSD.csv.gz
    data_path = Path("XBTUSD.csv.gz")

    if not data_path.exists():
        raise FileNotFoundError(
            f"Tardis data file not found: {data_path}\n"
            "Download the free XBTUSD quote data:\n\n"
            "  curl -LO https://datasets.tardis.dev/v1/bitmex/quotes/2024/01/01/XBTUSD.csv.gz\n\n"
            "Then re-run this script from the same directory.",
        )

    loader = TardisCSVDataLoader(instrument_id=instrument_id)
    quotes = loader.load_quotes(data_path)

    # XBTUSD: inverse perpetual, BTC-margined, 1 contract = 1 USD notional.
    # Price tick: $0.5. Check https://www.bitmex.com/app/contract/XBTUSD for current rates.
    XBTUSD = PerpetualContract(
        instrument_id=instrument_id,
        raw_symbol=Symbol("XBTUSD"),
        underlying="XBT",
        asset_class=AssetClass.CRYPTOCURRENCY,
        base_currency=BTC,
        quote_currency=USD,
        settlement_currency=BTC,
        is_inverse=True,
        price_precision=1,
        size_precision=0,
        price_increment=Price.from_str("0.5"),
        size_increment=Quantity.from_int(1),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_int(1),
        margin_init=Decimal("0.01"),
        margin_maint=Decimal("0.005"),
        maker_fee=Decimal("-0.00025"),
        taker_fee=Decimal("0.00075"),
        ts_event=0,
        ts_init=0,
    )

    config = BacktestEngineConfig(
        trader_id=TraderId("BACKTESTER-001"),
        logging=LoggingConfig(log_level="INFO"),
    )

    engine = BacktestEngine(config=config)

    BITMEX = Venue("BITMEX")
    engine.add_venue(
        venue=BITMEX,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=BTC,
        starting_balances=[Money(1, BTC)],
    )

    engine.add_instrument(XBTUSD)
    engine.add_data(quotes)

    strategy_config = GridMarketMakerConfig(
        instrument_id=instrument_id,
        max_position=Quantity.from_int(300),
        trade_size=Quantity.from_int(100),
        num_levels=3,
        grid_step_bps=100,
        skew_factor=0.5,
        requote_threshold_bps=10,
    )
    strategy = GridMarketMaker(config=strategy_config)
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
        print(engine.trader.generate_account_report(BITMEX))
        print(engine.trader.generate_order_fills_report())
        print(engine.trader.generate_positions_report())

    engine.reset()
    engine.dispose()
