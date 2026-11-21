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
Backtest order book imbalance on Polymarket book history from FinFeed.

Companion program for the "Backtest with Polymarket Order Book Data (FinFeed)" tutorial.
The first run downloads the book history and reads `FINFEED_API_KEY` or prompts for it.
Later runs reuse the Parquet catalog.

"""

import asyncio
import os
from datetime import UTC
from datetime import datetime
from decimal import Decimal
from getpass import getpass
from pathlib import Path

from nautilus_trader.adapters.finfeed import FinFeedDataLoader
from nautilus_trader.adapters.finfeed import finfeed_instrument_id
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.common import LogLevel
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import DataCatalogConfig
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import LoggerConfig
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import BinaryOption
from nautilus_trader.model import BookType
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import Quantity
from nautilus_trader.model import RecordFlag
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.trading import OrderBookImbalanceConfig


MARKET_ID = "BITCOIN-ABOVE-76K-ON-NOVEMBER-3-2026_NO"
INSTRUMENT_ID = finfeed_instrument_id("POLYMARKET", MARKET_ID)
START = datetime(2026, 11, 2, 20, 0, 5, tzinfo=UTC)
END = datetime(2026, 11, 2, 20, 2, tzinfo=UTC)
BOOK_TYPE = BookType.L2_MBP

DATA_DIR = Path(os.environ.get("NAUTILUS_DATA_DIR", "~/Downloads/Data")).expanduser() / "FinFeed"
CATALOG_PATH = DATA_DIR / "btc-above-76k-nov3-no-20261102-20"


async def download(api_key: str) -> tuple[BinaryOption, list[OrderBookDelta]]:
    """
    Download the instrument definition, then the book deltas for the window.
    """
    loader = FinFeedDataLoader(api_key=api_key)
    instrument = await loader.request_instrument(INSTRUMENT_ID)
    deltas = await loader.request_book_deltas(INSTRUMENT_ID, start=START, end=END)
    return instrument, deltas


if __name__ == "__main__":
    if not CATALOG_PATH.exists():
        api_key = os.environ.get("FINFEED_API_KEY") or getpass("FinFeed API key: ")
        instrument, deltas = asyncio.run(download(api_key))
        del api_key

        CATALOG_PATH.mkdir(parents=True)
        catalog = ParquetDataCatalog(str(CATALOG_PATH))
        catalog.write_instruments([instrument])
        catalog.write_order_book_deltas(deltas)

    catalog = ParquetDataCatalog(str(CATALOG_PATH))
    [instrument] = catalog.instruments(instrument_ids=[str(INSTRUMENT_ID)])
    deltas = catalog.query_order_book_deltas(identifiers=[str(INSTRUMENT_ID)])
    print(f"Catalog: {CATALOG_PATH}")
    print(f"Instrument: {instrument.id}, outcome: {instrument.outcome}")

    book = OrderBook(INSTRUMENT_ID, BOOK_TYPE)
    batches = 0

    for delta in deltas:
        book.apply_delta(delta)
        if RecordFlag.F_LAST.matches(delta.flags):
            book.check_integrity()
            batches += 1

    print(f"Loaded {len(deltas):,} deltas in {batches:,} batches")
    print(book.pprint(3))

    config = BacktestRunConfig(
        engine=BacktestEngineConfig(
            logging=LoggerConfig(stdout_level=LogLevel.ERROR),
            data_engine=DataEngineConfig(emit_quotes_from_book=True),
        ),
        venues=[
            BacktestVenueConfig(
                name=str(INSTRUMENT_ID.venue),
                oms_type=OmsType.NETTING,
                account_type=AccountType.CASH,
                base_currency=instrument.currency,
                starting_balances=["1000 pUSD"],
                book_type=BOOK_TYPE,
                fee_model=MakerTakerFeeModel(maker_rate=Decimal(0), taker_rate=Decimal(0)),
            ),
        ],
        data=[
            BacktestDataConfig(
                catalog=DataCatalogConfig(path=str(CATALOG_PATH)),
                data_type=NautilusDataType.OrderBookDelta,
                instrument_id=instrument.id,
            ),
        ],
        dispose_on_completion=False,
        raise_exception=True,
    )

    strategy_config = OrderBookImbalanceConfig(
        instrument_id=instrument.id,
        book_type=BOOK_TYPE,
        max_trade_size=Quantity.from_str("1.000000"),
        trigger_min_size=Decimal("100.0"),
        trigger_imbalance_ratio=Decimal("0.20"),
        min_seconds_between_triggers=1.0,
    )

    node = BacktestNode(configs=[config])

    try:
        node.build()
        node.add_builtin_strategy(config.id, "OrderBookImbalance", strategy_config)
        results = node.run()

        orders = node.generate_orders_report(config.id)
        executions = node.generate_fills_report(config.id)
        positions = node.generate_positions_report(config.id)
        account = node.generate_account_report(config.id, venue=INSTRUMENT_ID.venue)
    finally:
        node.dispose()

    print(results)
    print(orders["status"].value_counts().to_string())
    print(executions.groupby(["order_type", "order_side"]).size().to_string())
    print(
        executions[["ts_event", "order_side", "order_type", "last_qty", "last_px", "commission"]]
        .tail(8)
        .to_string(),
    )
    print(
        positions[
            ["side", "quantity", "peak_qty", "avg_px_open", "avg_px_close", "realized_pnl"]
        ].to_string(),
    )
    print(account[["total", "locked", "free", "currency"]].tail().to_string())

    assert not positions.empty, "No positions opened; inspect the selected window and signal"
    assert positions["ts_closed"].notna().all(), "The strategy left an open position"
    assert positions["side"].eq("FLAT").all()
    assert positions["quantity"].map(Decimal).eq(Decimal(0)).all()
