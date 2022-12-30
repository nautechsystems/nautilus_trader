import itertools
import os

from nautilus_trader.persistence.catalog.rust.reader import ParquetBufferReader
from nautilus_trader.model.data.tick import QuoteTick
from tests import TEST_DATA_DIR

# build and load pyo3 module using maturin
from nautilus_persistence import ParquetType, ParquetReader, ParquetReaderType

def test_cython_parquet_buffer_reader(file_data: bytes):
    reader = ParquetBufferReader(file_data, QuoteTick)
    ticks = list(itertools.chain(*list(reader)))

    print(len(ticks))


def test_pyo3_parquet_buffer_reader(file_data: bytes):
    reader = ParquetReader("", 1000, ParquetType.QuoteTick, ParquetReaderType.Buffer, file_data)
    data = map(lambda chunk: QuoteTick.list_from_capsule(chunk), reader)
    ticks = list(itertools.chain(*data))

    print(len(ticks))

def test_benchmark_buffer_parquet_reader(benchmark):
    def setup():
        parquet_data_path = os.path.join(TEST_DATA_DIR, "quote_tick_data.parquet")
        with open(parquet_data_path, "rb") as f:
            return f.read(), {}
        
    benchmark.pedantic(test_cython_parquet_buffer_reader, rounds=10, warmup_rounds=5, setup=setup)
    benchmark.pedantic(test_pyo3_parquet_buffer_reader, rounds=10, warmup_rounds=5, setup=setup)
