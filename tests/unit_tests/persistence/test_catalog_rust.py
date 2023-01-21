# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
import time

import pandas as pd
import pytest

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.data.wranglers import QuoteTickDataWrangler

# build and load pyo3 module using maturin
from nautilus_trader.core.nautilus import persistence
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.rust.reader import ParquetBufferReader
from nautilus_trader.persistence.catalog.rust.reader import ParquetFileReader
from nautilus_trader.persistence.catalog.rust.writer import ParquetWriter
from tests import TEST_DATA_DIR


def test_parquet_writer_vs_legacy_wrangler():
    # Arrange: Load CSV quote ticks
    df = pd.read_csv(
        os.path.join(TEST_DATA_DIR, "quote_tick_data.csv"),
        header=None,
        names=["ts_init", "bid", "ask", "volume"],
    ).set_index("ts_init")
    df.index = pd.to_datetime(df.index, format="%Y%m%d %H%M%S%f", utc=True)
    wrangler = QuoteTickDataWrangler(TestInstrumentProvider.default_fx_ccy("EUR/USD"))
    quotes = wrangler.process(data=df)

    # Write to parquet
    file_path = os.path.join(os.getcwd(), "quote_test1.parquet")
    if os.path.exists(file_path):
        os.remove(file_path)
    metadata = {"instrument_id": "EUR/USD.SIM", "price_precision": "5", "size_precision": "0"}
    writer = ParquetWriter(QuoteTick, metadata)

    writer.write(quotes)
    data = writer.flush()
    with open(file_path, "wb") as f:
        f.write(data)

    # Act
    reader = ParquetFileReader(QuoteTick, file_path)
    ticks = list(itertools.chain(*list(reader)))

    # Assert
    assert len(ticks) == len(quotes)
    assert ticks[0] == quotes[0]
    assert ticks[-1] == quotes[-1]

    # Clean up
    if os.path.exists(file_path):
        os.remove(file_path)


@pytest.mark.benchmark(
    group="parquet-reader",
    timer=time.time,
    disable_gc=True,
    min_rounds=5,
    warmup=False,
)
def test_parquet_reader_quote_ticks(benchmark):
    @benchmark
    def get_ticks():
        parquet_data_path = os.path.join(TEST_DATA_DIR, "quote_tick_data.parquet")
        reader = ParquetFileReader(QuoteTick, parquet_data_path)

        return list(itertools.chain(*list(reader)))

    ticks = get_ticks
    csv_data_path = os.path.join(TEST_DATA_DIR, "quote_tick_data.csv")
    df = pd.read_csv(csv_data_path, header=None, names="dates bid ask bid_size".split())

    assert len(ticks) == len(df)
    assert df.bid.equals(pd.Series(float(tick.bid) for tick in ticks))
    assert df.ask.equals(pd.Series(float(tick.ask) for tick in ticks))
    # TODO Sizes are off: mixed precision in csv
    assert df.bid_size.equals(pd.Series(int(tick.bid_size) for tick in ticks))
    # TODO Dates are off: test data timestamps use ms instead of ns...
    # assert df.dates.equals(
    #     pd.Series([unix_nanos_to_dt(tick.ts_init).strftime("%Y%m%d %H%M%S%f") for tick in ticks]),
    # )


def test_buffer_parquet_reader_quote_ticks():
    parquet_data_path = os.path.join(TEST_DATA_DIR, "quote_tick_data.parquet")
    reader = None

    with open(parquet_data_path, "rb") as f:
        data = f.read()
        reader = ParquetBufferReader(data, QuoteTick)

    ticks = list(itertools.chain(*list(reader)))

    csv_data_path = os.path.join(TEST_DATA_DIR, "quote_tick_data.csv")
    df = pd.read_csv(csv_data_path, header=None, names="dates bid ask bid_size".split())

    assert len(ticks) == len(df)
    assert df.bid.equals(pd.Series(float(tick.bid) for tick in ticks))
    assert df.ask.equals(pd.Series(float(tick.ask) for tick in ticks))
    # TODO Sizes are off: mixed precision in csv
    assert df.bid_size.equals(pd.Series(int(tick.bid_size) for tick in ticks))
    # TODO Dates are off: test data timestamps use ms instead of ns...
    # assert df.dates.equals(
    #     pd.Series([unix_nanos_to_dt(tick.ts_init).strftime("%Y%m%d %H%M%S%f") for tick in ticks]),
    # )


@pytest.mark.benchmark(
    group="parquet-reader",
    timer=time.time,
    disable_gc=True,
    min_rounds=5,
    warmup=False,
)
def test_pyo3_parquet_reader_quote_ticks(benchmark):
    @benchmark
    def get_ticks():
        parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_data/quote_tick_data.parquet")
        reader = persistence.ParquetReader(
            parquet_data_path,
            1000,
            persistence.ParquetType.QuoteTick,
            persistence.ParquetReaderType.File,
        )

        data = map(lambda chunk: QuoteTick.list_from_capsule(chunk), reader)
        ticks = list(itertools.chain(*data))
        return ticks

    ticks = get_ticks
    csv_data_path = os.path.join(TEST_DATA_DIR, "quote_tick_data.csv")
    df = pd.read_csv(csv_data_path, header=None, names="dates bid ask bid_size".split())

    assert len(ticks) == len(df)
    assert df.bid.equals(pd.Series(float(tick.bid) for tick in ticks))
    assert df.ask.equals(pd.Series(float(tick.ask) for tick in ticks))
    # TODO Sizes are off: mixed precision in csv
    assert df.bid_size.equals(pd.Series(int(tick.bid_size) for tick in ticks))
    # TODO Dates are off: test data timestamps use ms instead of ns...
    # assert df.dates.equals(
    #     pd.Series([unix_nanos_to_dt(tick.ts_init).strftime("%Y%m%d %H%M%S%f") for tick in ticks]),
    # )


@pytest.mark.skip(reason="WIP")
def test_pyo3_buffer_parquet_reader_quote_ticks():
    parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_data/quote_tick_data.parquet")
    reader = None
    with open(parquet_data_path, "rb") as f:
        data = f.read()
        reader = persistence.ParquetReader(
            "",
            1000,
            persistence.ParquetType.QuoteTick,
            persistence.ParquetReaderType.Buffer,
            data,
        )

    data = map(lambda chunk: QuoteTick.list_from_capsule(chunk), reader)
    ticks = list(itertools.chain(*data))

    csv_data_path = os.path.join(TEST_DATA_DIR, "quote_tick_data.csv")
    df = pd.read_csv(csv_data_path, header=None, names="dates bid ask bid_size".split())

    assert len(ticks) == len(df)
    assert df.bid.equals(pd.Series(float(tick.bid) for tick in ticks))
    assert df.ask.equals(pd.Series(float(tick.ask) for tick in ticks))
    # TODO Sizes are off: mixed precision in csv
    assert df.bid_size.equals(pd.Series(int(tick.bid_size) for tick in ticks))
    # TODO Dates are off: test data timestamps use ms instead of ns...
    # assert df.dates.equals(
    #     pd.Series([unix_nanos_to_dt(tick.ts_init).strftime("%Y%m%d %H%M%S%f") for tick in ticks]),
    # )


def test_parquet_writer_round_trip_quote_ticks():
    # Arrange
    n = 16384
    ticks = [
        QuoteTick(
            InstrumentId.from_str("EUR/USD.SIM"),
            Price(1.234, 4),
            Price(1.234, 4),
            Quantity(5, 0),
            Quantity(5, 0),
            0,
            0,
        )
        for _ in range(n)
    ]

    file_path = os.path.join(os.getcwd(), "quote_test3.parquet")
    if os.path.exists(file_path):
        os.remove(file_path)

    metadata = {
        "instrument_id": "EUR/USD.SIM",
        "price_precision": "4",
        "size_precision": "0",
    }

    writer = ParquetWriter(QuoteTick, metadata)
    writer.write(ticks)

    data = writer.flush()

    with open(file_path, "wb") as f:
        f.write(data)

    # Act
    reader = ParquetFileReader(QuoteTick, file_path)
    read_ticks = list(itertools.chain(*list(reader)))

    # Assert
    assert len(ticks) == n
    assert ticks == read_ticks

    # Cleanup
    os.remove(file_path)


def test_parquet_writer_round_trip_trade_ticks():
    # Arrange
    n = 16384
    ticks = [
        TradeTick(
            InstrumentId.from_str("EUR/USD.SIM"),
            Price(1.234, 4),
            Quantity(5, 4),
            AggressorSide.BUYER,
            TradeId("123456"),
            0,
            0,
        )
        for _ in range(n)
    ]
    file_path = os.path.join(os.getcwd(), "trade_test.parquet")
    if os.path.exists(file_path):
        os.remove(file_path)

    metadata = {
        "instrument_id": "EUR/USD.SIM",
        "price_precision": "4",
        "size_precision": "4",
    }

    writer = ParquetWriter(TradeTick, metadata)
    writer.write(ticks)

    data = writer.flush()
    with open(file_path, "wb") as f:
        f.write(data)

    # Act
    reader = ParquetFileReader(TradeTick, file_path)
    read_ticks = list(itertools.chain(*list(reader)))

    # Assert
    assert len(ticks) == n
    assert ticks == read_ticks

    # Cleanup
    os.remove(file_path)


def get_peak_memory_usage_gb():
    import platform

    BYTES_IN_GIGABYTE = 1e9
    if platform.system() == "Darwin" or platform.system() == "Linux":
        import resource

        return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / BYTES_IN_GIGABYTE
    elif platform.system() == "Windows":
        import psutil

        return psutil.Process().memory_info().peak_wset / BYTES_IN_GIGABYTE
    else:
        raise RuntimeError("Unsupported OS.")


@pytest.mark.skip(reason="takes too long")
def test_parquet_reader_frees_rust_memory():
    """
    The peak memory usage should not increase much more than the batch size
    when iterating the batches.
    """
    import gc

    # Arrange
    n = 16384
    ticks = [
        QuoteTick(
            InstrumentId.from_str("EUR/USD.SIM"),
            Price(1.234, 4),
            Price(1.234, 4),
            Quantity(5, 0),
            Quantity(5, 0),
            0,
            0,
        )
        for _ in range(n)
    ]

    file_path = os.path.join(os.getcwd(), "quote_test3.parquet")
    if os.path.exists(file_path):
        os.remove(file_path)

    metadata = {
        "instrument_id": "EUR/USD.SIM",
        "price_precision": "4",
        "size_precision": "0",
    }

    writer = ParquetWriter(QuoteTick, metadata)
    writer.write(ticks)

    data = writer.flush()

    with open(file_path, "wb") as f:
        f.write(data)

    # Act
    start_memory = get_peak_memory_usage_gb()
    print(f"{start_memory:2f}")

    for _ in range(1_000):
        reader = ParquetFileReader(QuoteTick, file_path)
        for _ in reader:
            pass
        gc.collect()
    gc.collect()

    end_memory = get_peak_memory_usage_gb()
    print(f"{end_memory:2f}")

    # Assert
    tolerance = 0.15
    assert start_memory - tolerance <= end_memory <= start_memory + tolerance
