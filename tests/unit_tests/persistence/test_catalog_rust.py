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
import tempfile

import pandas as pd

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetReader
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetReaderType
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetType
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetWriter
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from tests import TEST_DATA_DIR


def test_file_parquet_reader_quote_ticks():
    parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_data/quote_tick_data.parquet")
    reader = ParquetReader(
        parquet_data_path,
        1000,
        ParquetType.QuoteTick,
        ParquetReaderType.File,
    )

    mapped_chunk = map(QuoteTick.list_from_capsule, reader)
    ticks = list(itertools.chain(*mapped_chunk))

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
    parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_data/quote_tick_data.parquet")
    data = None

    with open(parquet_data_path, "rb") as f:
        data = f.read()

    reader = ParquetReader(
        "",
        1000,
        ParquetType.QuoteTick,
        ParquetReaderType.Buffer,
        data,
    )

    # Note: Naming the variable data gives an error
    # because somehow the iteration terminates after
    # 1 step. Something related to the variable data
    # being passed to reader and map function being lazy
    mapped_chunk = map(QuoteTick.list_from_capsule, reader)
    ticks = list(itertools.chain(*mapped_chunk))

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


def test_file_parquet_writer_quote_ticks():
    parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_data/quote_tick_data.parquet")

    # Write quotes
    reader = ParquetReader(
        parquet_data_path,
        1000,
        ParquetType.QuoteTick,
        ParquetReaderType.File,
    )

    metadata = {
        "instrument_id": "EUR/USD.SIM",
        "price_precision": "5",
        "size_precision": "0",
    }
    writer = ParquetWriter(
        ParquetType.QuoteTick,
        metadata,
    )

    file_path = tempfile.mktemp()

    for chunk in reader:
        writer.write(chunk)

    with open(file_path, "wb") as f:
        data: bytes = writer.flush_bytes()
        f.write(data)

    # Read quotes again
    reader = ParquetReader(
        file_path,
        1000,
        ParquetType.QuoteTick,
        ParquetReaderType.File,
    )

    # Cleanup
    os.remove(file_path)

    mapped_chunk = map(QuoteTick.list_from_capsule, reader)
    quotes = list(itertools.chain(*mapped_chunk))

    assert len(quotes) == 9500


def test_file_parquet_writer_trade_ticks():
    # Read quotes
    parquet_data_path = os.path.join(TEST_DATA_DIR, "trade_tick_data.parquet")
    assert os.path.exists(parquet_data_path)

    reader = ParquetReader(
        parquet_data_path,
        100,
        ParquetType.TradeTick,
        ParquetReaderType.File,
    )

    # Write trades
    metadata = {
        "instrument_id": "EUR/USD.SIM",
        "price_precision": "5",
        "size_precision": "0",
    }
    writer = ParquetWriter(
        ParquetType.QuoteTick,
        metadata,
    )

    file_path = tempfile.mktemp()
    with open(file_path, "wb") as f:
        for chunk in reader:
            writer.write(chunk)
        data: bytes = writer.flush_bytes()
        f.write(data)

    # Read quotes again
    reader = ParquetReader(
        parquet_data_path,
        100,
        ParquetType.TradeTick,
        ParquetReaderType.File,
    )

    # Cleanup
    os.remove(file_path)

    mapped_chunk = map(TradeTick.list_from_capsule, reader)
    trades = list(itertools.chain(*mapped_chunk))

    assert len(trades) == 100


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


# @pytest.mark.skip(reason="takes too long")
# def test_parquet_reader_frees_rust_memory():
#     """
#     The peak memory usage should not increase much more than the batch size
#     when iterating the batches.
#     """
#     import gc
#
#     # Arrange
#     n = 16384
#     ticks = [
#         QuoteTick(
#             InstrumentId.from_str("EUR/USD.SIM"),
#             Price(1.234, 4),
#             Price(1.234, 4),
#             Quantity(5, 0),
#             Quantity(5, 0),
#             0,
#             0,
#         )
#         for _ in range(n)
#     ]
#
#     file_path = os.path.join(os.getcwd(), "quote_test3.parquet")
#     if os.path.exists(file_path):
#         os.remove(file_path)
#
#     metadata = {
#         "instrument_id": "EUR/USD.SIM",
#         "price_precision": "4",
#         "size_precision": "0",
#     }
#
#     writer = persistence.ParquetWriter(QuoteTick, metadata)
#     writer.write(ticks)
#
#     data = writer.flush()
#
#     with open(file_path, "wb") as f:
#         f.write(data)
#
#     # Act
#     start_memory = get_peak_memory_usage_gb()
#     print(f"{start_memory:2f}")
#
#     for _ in range(1_000):
#         reader = ParquetFileReader(QuoteTick, file_path)
#         for _ in reader:
#             pass
#         gc.collect()
#     gc.collect()
#
#     end_memory = get_peak_memory_usage_gb()
#     print(f"{end_memory:2f}")
#
#     # Assert
#     tolerance = 0.15
#     assert start_memory - tolerance <= end_memory <= start_memory + tolerance
