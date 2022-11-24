# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import itertools
import os

import pandas as pd

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.data.wranglers import QuoteTickDataWrangler
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.rust.reader import ParquetFileReader
from nautilus_trader.persistence.catalog.rust.writer import ParquetWriter


def test_parquet_writer_vs_legacy_wrangler():
    # Load CSV quote ticks
    df = pd.read_csv(
        os.path.join(PACKAGE_ROOT, "tests/test_kit/data/quote_tick_data.csv"),
        header=None,
        names=["ts_init", "bid", "ask", "volume"],
    ).set_index("ts_init")
    df.index = pd.to_datetime(df.index, format="%Y%m%d %H%M%S%f", utc=True)
    wrangler = QuoteTickDataWrangler(TestInstrumentProvider.default_fx_ccy("EUR/USD"))
    quotes = wrangler.process(data=df)

    # Write to parquet
    file_path = os.path.join(os.getcwd(), "quote_test.parquet")
    if os.path.exists(file_path):
        os.remove(file_path)
    metadata = {"instrument_id": "EUR/USD.SIM", "price_precision": "5", "size_precision": "0"}
    writer = ParquetWriter(QuoteTick, metadata)

    writer.write(quotes[:8192])
    data = writer.flush()

    with open(file_path, "wb") as f:
        f.write(data)

    # Ensure we're reading the same ticks back
    # reader = ParquetFileReader(QuoteTick, file_path)
    # ticks = list(itertools.chain(*list(reader)))
    # assert len(ticks) == len(quotes)
    # assert ticks[0] == quotes[0]
    # assert ticks[-1] == quotes[-1]

    # Clean up
    file_path = os.path.join(os.getcwd(), "quote_test.parquet")
    if os.path.exists(file_path):
        os.remove(file_path)


def test_parquet_reader_quote_ticks():
    parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_kit/data/quote_tick_data.parquet")
    reader = ParquetFileReader(QuoteTick, parquet_data_path)

    ticks = list(itertools.chain(*list(reader)))

    csv_data_path = os.path.join(PACKAGE_ROOT, "tests/test_kit/data/quote_tick_data.csv")
    df = pd.read_csv(csv_data_path, header=None, names="dates bid ask bid_size".split())

    assert len(ticks) == len(df)
    assert df.bid.equals(pd.Series(float(tick.bid) for tick in ticks))
    assert df.ask.equals(pd.Series(float(tick.ask) for tick in ticks))
    # TODO Sizes are off: mixed precision in csv
    # assert df.bid_size.equals(pd.Series(int(tick.bid_size) for tick in ticks))

    # TODO Dates are off: test data timestamps use ms instead of ns...
    # assert df.dates.equals(pd.Series([unix_nanos_to_dt(tick.ts_init).strftime("%Y%m%d %H%M%S%f") for tick in ticks]))


def test_parquet_writer_round_trip_quote_ticks():
    n = 8092
    ticks = [
        QuoteTick(
            InstrumentId.from_str("EUR/USD.SIM"),
            Price(1.234, 4),
            Price(1.234, 4),
            Quantity(5, 0),
            Quantity(5, 0),
            0,
            0,
        ),
    ] * n
    file_path = os.path.join(os.getcwd(), "quote_test.parquet")
    if os.path.exists(file_path):
        os.remove(file_path)
    metadata = {"instrument_id": "EUR/USD.SIM", "price_precision": "5", "size_precision": "0"}
    writer = ParquetWriter(QuoteTick, metadata)
    writer.write(ticks)

    data = writer.flush()
    with open(file_path, "wb") as f:
        f.write(data)

    assert os.path.exists(file_path)
    reader = ParquetFileReader(QuoteTick, file_path)
    ticks = list(itertools.chain(*list(reader)))

    assert len(ticks) == n

    # Cleanup
    os.remove(file_path)


def test_parquet_writer_round_trip_trade_ticks():
    n = 8092
    ticks = [
        TradeTick(
            InstrumentId.from_str("EUR/USD.SIM"),
            Price(1.234, 4),
            Quantity(5, 4),
            AggressorSide.BUY,
            TradeId("123456"),
            0,
            0,
        ),
    ] * n
    file_path = os.path.join(os.getcwd(), "trade_test.parquet")
    if os.path.exists(file_path):
        os.remove(file_path)
    metadata = {"instrument_id": "EUR/USD.SIM", "price_precision": "4", "size_precision": "4"}
    writer = ParquetWriter(TradeTick, metadata)
    writer.write(ticks)

    data = writer.flush()
    with open(file_path, "wb") as f:
        f.write(data)

    assert os.path.exists(file_path)

    # Act
    reader = ParquetFileReader(TradeTick, file_path)
    ticks = list(itertools.chain(*list(reader)))

    # Assert
    assert len(ticks) == n

    # Cleanup
    os.remove(file_path)
