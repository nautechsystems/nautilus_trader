import os

from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.rust.writer import ParquetWriter


if __name__ == "__main__":
    n = 100
    ticks = [
        QuoteTick(
            InstrumentId.from_str("EUR/USD.DUKA"),
            Price(1.234, 4),
            Price(1.234, 4),
            Quantity(5, 0),
            Quantity(5, 0),
            0,
            0,
        )
    ] * n

    file_path = os.path.expanduser("~/Desktop/test_parquet_writer.parquet")
    writer = ParquetWriter(file_path, QuoteTick)
    writer.write(ticks)
