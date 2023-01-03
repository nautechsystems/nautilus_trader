import os

from nautilus_persistence import ParquetReader
from nautilus_persistence import ParquetReaderType
from nautilus_persistence import ParquetType

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.model.data.tick import QuoteTick


def test_python_parquet_reader():
    parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_data/quote_tick_data.parquet")
    reader = ParquetReader(parquet_data_path, 100, ParquetType.QuoteTick, ParquetReaderType.File)

    total_count = 0
    for chunk in reader:
        tick_list = QuoteTick.list_from_capsule(chunk)
        total_count += len(tick_list)

    reader.drop()

    assert total_count == 9500
    # test on last chunk tick i.e. 9500th record
    assert str(tick_list[-1]) == "EUR/USD.SIM,1.12130,1.12132,0,0,1577919652000000125"


if __name__ == "__main__":
    test_python_parquet_reader()
