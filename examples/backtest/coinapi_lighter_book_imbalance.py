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
Backtest order book imbalance on Lighter ANTHROPIC pre-IPO book data from CoinAPI.

Companion program for the "Backtest with Pre-IPO Order Book Data (CoinAPI)" tutorial.
The first run downloads one Flat Files export and reads `COINAPI_API_KEY` or prompts for it.
Later runs reuse the Parquet catalog.

"""

import asyncio
import os
from decimal import Decimal
from getpass import getpass
from hashlib import file_digest
from pathlib import Path

from nautilus_trader.adapters.coinapi import CoinApiDataLoader
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
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookType
from nautilus_trader.model import CryptoPerpetual
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderBook
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import RecordFlag
from nautilus_trader.model import StandardMarginModel
from nautilus_trader.model import Symbol
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.trading import OrderBookImbalanceConfig


BUCKET = "coinapi"
OBJECT_KEY = (
    "T-LIMITBOOK_FULL/D-2026100100/E-LIGHTER/"
    "IDDI-47534074+SC-LIGHTER_PERP_ANTHROPIC_USD+S-ANTHROPIC.csv.gz"
)
FILE_SHA256 = "2ed141be6480c56ff16322e08bbc43398edc710875fe81093e0635ff672e48e4"
INSTRUMENT_ID = InstrumentId.from_str("ANTHROPIC-PERP.LIGHTER")
BOOK_TYPE = BookType.L2_MBP

DATA_DIR = Path(os.environ.get("NAUTILUS_DATA_DIR", "~/Downloads/Data")).expanduser() / "CoinAPI"
FILEPATH = DATA_DIR / OBJECT_KEY
CATALOG_PATH = DATA_DIR / "lighter-anthropic-20261001-00"


async def download(api_key: str) -> None:
    """
    Download the selected Flat Files export to `FILEPATH`.
    """
    loader = CoinApiDataLoader(api_key=api_key)
    await loader.download_flat_file(BUCKET, OBJECT_KEY, FILEPATH)


if __name__ == "__main__":
    if not CATALOG_PATH.exists():
        if not FILEPATH.is_file():
            api_key = os.environ.get("COINAPI_API_KEY") or getpass("CoinAPI API key: ")
            asyncio.run(download(api_key))
            del api_key

        with FILEPATH.open("rb") as file:
            checksum = file_digest(file, "sha256").hexdigest()

        if checksum != FILE_SHA256:
            raise ValueError(f"Unexpected SHA-256 for {FILEPATH}, verify the selected export")

        usdc = Currency.from_str("USDC")

        instrument = CryptoPerpetual(
            instrument_id=INSTRUMENT_ID,
            raw_symbol=Symbol("ANTHROPIC"),
            base_currency=Currency.from_str("ANTHROPIC"),
            quote_currency=usdc,
            settlement_currency=usdc,
            is_inverse=False,
            price_precision=1,
            size_precision=5,
            price_increment=Price.from_str("0.1"),
            size_increment=Quantity.from_str("0.00001"),
            min_quantity=Quantity.from_str("0.00250"),
            min_notional=Money.from_decimal(Decimal(10), usdc),
            margin_init=Decimal("0.20"),
            margin_maint=Decimal("0.12"),
            ts_event=0,
            ts_init=0,
        )

        loader = CoinApiDataLoader()
        loader.add_instrument(instrument)
        deltas = loader.load_book_deltas(FILEPATH, INSTRUMENT_ID)

        CATALOG_PATH.mkdir(parents=True)
        catalog = ParquetDataCatalog(str(CATALOG_PATH))
        catalog.write_instruments([instrument])
        catalog.write_order_book_deltas(deltas)

    catalog = ParquetDataCatalog(str(CATALOG_PATH))
    [instrument] = catalog.instruments(instrument_ids=[str(INSTRUMENT_ID)])
    deltas = catalog.query_order_book_deltas(identifiers=[str(INSTRUMENT_ID)])
    print(f"Catalog: {CATALOG_PATH}")
    print(
        f"Instrument: {instrument.id}, price precision {instrument.price_precision}, "
        f"size precision {instrument.size_precision}",
    )

    first = deltas[0]
    if first.action != BookAction.CLEAR or not RecordFlag.F_SNAPSHOT.matches(first.flags):
        raise ValueError("Book deltas do not open with a snapshot")

    book = OrderBook(INSTRUMENT_ID, BOOK_TYPE)
    batches = 0
    locked_or_crossed = 0

    for delta in deltas:
        book.apply_delta(delta)
        if not RecordFlag.F_LAST.matches(delta.flags):
            continue

        batches += 1

        if RecordFlag.F_SNAPSHOT.matches(delta.flags):
            book.check_integrity()

        bid, ask = book.best_bid_price(), book.best_ask_price()
        if bid is not None and ask is not None and bid.as_decimal() >= ask.as_decimal():
            locked_or_crossed += 1

    book.check_integrity()
    print(f"Loaded {len(deltas):,} deltas in {batches:,} batches")
    print(f"Locked or crossed books: {locked_or_crossed:,}")
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
                account_type=AccountType.MARGIN,
                starting_balances=["100000 USDC"],
                book_type=BOOK_TYPE,
                margin_model=StandardMarginModel(),
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
        max_trade_size=Quantity.from_str("0.01000"),
        trigger_min_size=Decimal("1.0"),
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
            [
                "side",
                "quantity",
                "peak_qty",
                "avg_px_open",
                "avg_px_close",
                "realized_pnl",
                "commissions",
            ]
        ].to_string(),
    )
    print(account[["total", "locked", "free", "currency"]].tail().to_string())

    assert not positions.empty, "No positions opened; inspect the selected file and signal"
    assert positions["ts_closed"].notna().all(), "The strategy left an open position"
    assert positions["side"].eq("FLAT").all()
    assert positions["quantity"].map(Decimal).eq(Decimal(0)).all()
