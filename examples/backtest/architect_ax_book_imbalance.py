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

import databento as db
import pandas as pd

from nautilus_trader.adapters.databento import DatabentoDataLoader
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import LoggingConfig
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalance
from nautilus_trader.examples.strategies.orderbook_imbalance import OrderBookImbalanceConfig
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


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

if __name__ == "__main__":
    instrument_id = InstrumentId.from_str("XAU-PERP.AX")
    data_path = Path("gc_gold_mbp1.dbn.zst")

    if not data_path.exists():
        client = db.Historical()
        data = client.timeseries.get_range(
            dataset="GLBX.MDP3",
            symbols=["GC.v.0"],
            stype_in="continuous",
            schema="mbp-1",
            start="2024-11-15",
            end="2024-11-16",
        )
        data.to_file(data_path)

    XAU_PERP = PerpetualContract(
        instrument_id=instrument_id,
        raw_symbol=Symbol("XAU-PERP"),
        underlying="XAU",
        asset_class=AssetClass.COMMODITY,
        quote_currency=USD,
        settlement_currency=USD,
        is_inverse=False,
        price_precision=2,
        size_precision=0,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_int(1),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_int(1),
        margin_init=Decimal("0.08"),
        margin_maint=Decimal("0.04"),
        maker_fee=Decimal("0.0002"),
        taker_fee=Decimal("0.0005"),
        ts_event=0,
        ts_init=0,
    )

    loader = DatabentoDataLoader()
    quotes = loader.from_dbn_file(
        path=data_path,
        instrument_id=instrument_id,
    )

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

    engine.add_instrument(XAU_PERP)
    engine.add_data(quotes)

    strategy_config = OrderBookImbalanceConfig(
        instrument_id=instrument_id,
        max_trade_size=Decimal(10),
        trigger_min_size=1.0,
        trigger_imbalance_ratio=0.10,
        min_seconds_between_triggers=5.0,
        book_type="L1_MBP",
        use_quote_ticks=True,
    )

    strategy = OrderBookImbalance(config=strategy_config)
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
