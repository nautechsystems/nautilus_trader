import os

from nautilus_persistence import ParquetType
from nautilus_persistence import PythonParquetReader

from nautilus_trader import PACKAGE_ROOT


def test_python_parquet_reader():
    parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_kit/data/quote_tick_data.parquet")
    reader = PythonParquetReader(parquet_data_path, 100, ParquetType.QuoteTick)

    for chunk in reader:
        print(chunk.len)
        reader.drop_chunk(chunk)

    reader.drop()


if __name__ == "__main__":
    test_python_parquet_reader()
