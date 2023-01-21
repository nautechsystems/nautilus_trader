import itertools
import os
import time

import pytest

# build and load pyo3 module using maturin
from nautilus.persistence import ParquetReader
from nautilus.persistence import ParquetReaderType
from nautilus.persistence import ParquetType

from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.persistence.catalog.rust.reader import ParquetBufferReader
from tests import TEST_DATA_DIR


@pytest.mark.benchmark(
    group="parquet-reader",
    min_rounds=5,
    timer=time.time,
    disable_gc=True,
    warmup=True,
)
def test_cython_benchmark_parquet_buffer_reader(benchmark):
    parquet_data_path = os.path.join(TEST_DATA_DIR, "quote_tick_data.parquet")
    file_data = None
    with open(parquet_data_path, "rb") as f:
        file_data = f.read()

    @benchmark
    def run():
        reader = ParquetBufferReader(file_data, QuoteTick)
        ticks = list(itertools.chain(*list(reader)))

        print(len(ticks))

    run


@pytest.mark.benchmark(
    group="parquet-reader",
    min_rounds=5,
    timer=time.time,
    disable_gc=True,
    warmup=True,
)
def test_pyo3_benchmark_parquet_buffer_reader(benchmark):
    parquet_data_path = os.path.join(TEST_DATA_DIR, "quote_tick_data.parquet")
    file_data = None
    with open(parquet_data_path, "rb") as f:
        file_data = f.read()

    @benchmark
    def run():
        reader = ParquetReader("", 1000, ParquetType.QuoteTick, ParquetReaderType.Buffer, file_data)
        data = map(lambda chunk: QuoteTick.list_from_capsule(chunk), reader)
        ticks = list(itertools.chain(*data))

        print(len(ticks))

    run
