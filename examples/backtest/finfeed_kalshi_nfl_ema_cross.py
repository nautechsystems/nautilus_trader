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
Replay a quarter of Kalshi NFL point spread trades from FinFeed.

Download once to a Parquet catalog, then replay the trades through a directional EMA
crossover actor. The actor records signals without submitting orders.

"""

import asyncio
import os
from datetime import UTC
from datetime import datetime
from getpass import getpass
from hashlib import sha256
from pathlib import Path
from tempfile import TemporaryDirectory

from nautilus_trader.adapters.finfeed import FinFeedDataLoader
from nautilus_trader.adapters.finfeed import finfeed_instrument_id
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.common import DataActor
from nautilus_trader.common import LogLevel
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import LoggerConfig
from nautilus_trader.core import dt_to_unix_nanos
from nautilus_trader.core import unix_nanos_to_iso8601
from nautilus_trader.execution import FixedFeeModel
from nautilus_trader.indicators import ExponentialMovingAverage
from nautilus_trader.model import AccountType
from nautilus_trader.model import BinaryOption
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import TradeTick
from nautilus_trader.persistence import ParquetDataCatalog


INSTRUMENT_ID = finfeed_instrument_id("KALSHI", "KXNFLSPREAD-26OCT18DALGB-GB8_YES")
START = datetime(2026, 10, 19, 0, 22, 29, tzinfo=UTC)
END = datetime(2026, 10, 19, 0, 59, tzinfo=UTC)
FAST_PERIOD = 5
SLOW_PERIOD = 15
DATA_DIR = Path(os.environ.get("NAUTILUS_DATA_DIR", "~/Downloads/Data")).expanduser() / "FinFeed"
CATALOG_PATH = DATA_DIR / "kalshi-dallas-green-bay-20261019-q1"


class TradeEmaCross(DataActor):
    """
    Record directional signals from the last traded price.
    """

    def __init__(self) -> None:
        """
        Initialize the moving averages and signal state.
        """
        super().__init__()
        self.fast = ExponentialMovingAverage(FAST_PERIOD)
        self.slow = ExponentialMovingAverage(SLOW_PERIOD)
        self.trade_count = 0
        self.crossovers: list[tuple[int, bool]] = []
        self.fast_was_above: bool | None = None

    def on_start(self) -> None:
        """
        Subscribe to the selected FinFeed outcome.
        """
        self.subscribe_trades(INSTRUMENT_ID)

    def on_trade(self, trade: TradeTick) -> None:
        """
        Evaluate the signal after incorporating the recorded trade.
        """
        self.trade_count += 1
        self.fast.handle_trade_tick(trade)
        self.slow.handle_trade_tick(trade)

        if not self.fast.initialized or not self.slow.initialized:
            return

        fast_above = self.fast.value > self.slow.value
        if self.fast_was_above is not None and fast_above != self.fast_was_above:
            self.crossovers.append((self.trade_count - 1, fast_above))
        self.fast_was_above = fast_above


async def download(catalog_path: Path, api_key: str) -> None:
    """
    Request and save the instrument and first-quarter trades.
    """
    loader = FinFeedDataLoader(api_key=api_key, timeout_secs=120)
    instrument = await loader.request_instrument(INSTRUMENT_ID)
    trades = await loader.request_trades(INSTRUMENT_ID, start=START, end=END)
    if not trades:
        raise ValueError(f"FinFeed returned no trades for {INSTRUMENT_ID} from {START} to {END}")

    catalog_path.parent.mkdir(parents=True, exist_ok=True)
    with TemporaryDirectory(dir=catalog_path.parent) as temp_dir:
        staged_path = Path(temp_dir) / "catalog"
        staged_path.mkdir()
        catalog = ParquetDataCatalog(str(staged_path))
        catalog.write_instruments([instrument])
        catalog.write_trades(trades)
        staged_path.rename(catalog_path)


def replay(instrument: BinaryOption, trades: list[TradeTick]) -> list[tuple[int, bool]]:
    """
    Run the signal actor on the catalog trades in a backtest engine.
    """
    engine = BacktestEngine(
        BacktestEngineConfig(
            logging=LoggerConfig(stdout_level=LogLevel.ERROR),
            run_analysis=False,
        ),
    )

    actor = TradeEmaCross()
    try:
        engine.add_venue(
            venue=instrument.id.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.CASH,
            starting_balances=[Money.from_str("1000 USD")],
            base_currency=instrument.currency,
            fee_model=FixedFeeModel(Money.from_str("0 USD")),
        )
        engine.add_instrument(instrument)
        engine.add_data(trades)
        engine.add_actor(actor)
        engine.run()

        if actor.trade_count != len(trades):
            raise RuntimeError(f"Replayed {actor.trade_count} of {len(trades)} catalog trades")
        return actor.crossovers
    finally:
        engine.dispose()


def main() -> None:
    """
    Load the catalog, run the replay, and print a crossover trace.
    """
    if not CATALOG_PATH.exists():
        api_key = os.environ.get("FINFEED_API_KEY") or getpass("FinFeed API key: ")
        asyncio.run(download(CATALOG_PATH, api_key))

    catalog = ParquetDataCatalog(str(CATALOG_PATH))
    instruments = catalog.instruments(instrument_ids=[str(INSTRUMENT_ID)])
    if len(instruments) != 1:
        raise ValueError(f"Expected one definition for {INSTRUMENT_ID} in {CATALOG_PATH}")

    trades = catalog.query_trades(
        identifiers=[str(INSTRUMENT_ID)],
        start=dt_to_unix_nanos(START),
        end=dt_to_unix_nanos(END),
    )

    if not trades:
        raise ValueError(f"No trades for {INSTRUMENT_ID} from {START} to {END} in {CATALOG_PATH}")

    fingerprint = sha256()
    for trade in trades:
        fingerprint.update(bytes(trade.to_json_bytes()))
        fingerprint.update(b"\n")

    crossovers = replay(instruments[0], trades)
    print(f"Catalog: {CATALOG_PATH}")
    print(f"Instrument: {INSTRUMENT_ID}")
    print(f"Replay window: {START.isoformat()} to {END.isoformat()}")
    print(f"Replay input SHA-256: {fingerprint.hexdigest()}")
    print(f"Trades: {len(trades)}, crossovers: {len(crossovers)}")
    if crossovers:
        index, fast_above = crossovers[0]
        print(f"First crossover, signal: {'long' if fast_above else 'flat'}")

        for trade in trades[index : index + 3]:
            ts = unix_nanos_to_iso8601(trade.ts_event)
            print(f"{ts}  {trade.trade_id}  {trade.price} USD  {trade.size} contracts")


if __name__ == "__main__":
    main()
