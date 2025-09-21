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

import os
from pathlib import Path

import requests

from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.actor import ActorConfig
from nautilus_trader.common.component import init_logging
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.test_kit.providers import TestInstrumentProvider


class OrderBookActor(Actor):
    def __init__(self, instrument: Instrument):
        self.instrument = instrument
        config = ActorConfig(component_id="order_book")
        super().__init__(config=config)

    def on_start(self):
        book_type = nautilus_pyo3.BookType.L2_MBP

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(self.instrument.id.value)
        self.order_book = nautilus_pyo3.OrderBook(pyo3_instrument_id, book_type)
        self.subscribe_order_book_deltas(
            self.instrument.id,
            book_type,
            managed=False,
            pyo3_conversion=True,
        )

    def on_order_book_deltas(self, order_book_deltas: nautilus_pyo3.OrderBookDeltas):
        self.order_book.apply_deltas(order_book_deltas)
        print(f"applied {len(order_book_deltas.deltas)} deltas")
        self.order_book.clear_stale_levels()
        bid_prc = self.order_book.best_bid_price()
        ask_prc = self.order_book.best_ask_price()

        if bid_prc is None or ask_prc is None:
            return

        if bid_prc > ask_prc:
            self.log.error(
                f"Invalid order book state {self.instrument.id} "
                f"best_bid: {bid_prc} best_ask: {ask_prc}",
            )
            self.log.error(f"\n{self.order_book.pprint(num_levels=10)}")
            raise

    def on_stop(self):
        pass


def download_tardis_csv(api_key: str, data_dir: Path):
    """
    Download sample Tardis CSV data using a direct URL if it doesn't already exist.
    """
    venue = "binance"
    data_type = "incremental_book_L2"
    date = "2025-05-01"
    symbol = "BTCUSDT"
    filename = f"{symbol}.csv.gz"

    # Construct the local path within the user-specified data_dir
    date_path = date.replace("-", "/")
    local_dir = data_dir / venue / data_type / date_path
    local_dir.mkdir(parents=True, exist_ok=True)
    file_path = local_dir / filename

    if file_path.exists():
        print(f"File already exists: {file_path}")
        return file_path

    # Construct the direct download URL
    url = f"https://datasets.tardis.dev/v1/{venue}/{data_type}/{date_path}/{filename}"

    print(f"Downloading Tardis CSV data from {url}...")
    headers = {"Authorization": f"Bearer {api_key}"}

    with requests.get(url, headers=headers, stream=True) as response:
        response.raise_for_status()
        with open(file_path, "wb") as f:
            for chunk in response.iter_content(chunk_size=8192):
                f.write(chunk)

    print(f"Downloaded to {file_path}")
    return file_path


def run():
    """
    Download Tardis data, loads it, and runs a backtest.
    """
    nautilus_pyo3.init_tracing()
    _guard = init_logging(level_stdout=LogLevel.INFO)

    api_key = os.getenv("TARDIS_API_KEY") or os.getenv("TM_API_KEY")
    if not api_key:
        raise ValueError("TARDIS_API_KEY or TM_API_KEY environment variable not set")

    data_dir = Path.home() / "Downloads" / "tardis_data"
    csv_filepath = download_tardis_csv(api_key, data_dir)

    print(f"Loading data {csv_filepath}")

    instrument = TestInstrumentProvider.btcusdt_binance()

    # Load the data
    loader = TardisCSVDataLoader(instrument_id=instrument.id)
    iterator = loader.stream_batched_deltas(
        filepath=csv_filepath,
        chunk_size=100_000,
    )

    # Setup backtest engine
    engine = BacktestEngine()

    engine.add_venue(
        venue=instrument.venue,
        oms_type=OmsType.NETTING,
        book_type=BookType.L2_MBP,
        account_type=AccountType.MARGIN,  # Spot CASH account (not for perpetuals or futures)
        base_currency=None,  # Multi-currency account
        starting_balances=[Money(1_000_000.0, USDT), Money(10.0, BTC)],
    )

    # data = []
    # for data_list in iterator:
    #     data.extend(data_list)

    engine.add_instrument(instrument)
    engine.add_data_iterator(str(csv_filepath), iterator)
    # engine.add_data(data)
    actor = OrderBookActor(instrument)
    engine.add_actor(actor)

    print("Running backtest...")
    engine.run()
    print("Backtest finished")


if __name__ == "__main__":
    run()
