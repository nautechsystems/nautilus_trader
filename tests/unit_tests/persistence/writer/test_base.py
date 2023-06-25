import os
import tempfile

from nautilus_trader.core.nautilus_pyo3.persistence import ParquetType
from nautilus_trader.core.nautilus_pyo3.persistence import PythonCatalog
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.parquet.writer import ParquetWriter
from nautilus_trader.persistence.catalog.parquet.writer import objects_to_table
from nautilus_trader.persistence.wranglers import list_from_capsule
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def test_writer_writes_quote_ticks_objects():
    instrument = TestInstrumentProvider.default_fx_ccy("GBP/USD")
    quotes = [
        QuoteTick(
            instrument_id=instrument.id,
            ask=Price.from_str("2.0"),
            bid=Price.from_str("2.1"),
            bid_size=Quantity.from_int(10),
            ask_size=Quantity.from_int(10),
            ts_event=0,
            ts_init=0,
        ),
        QuoteTick(
            instrument_id=instrument.id,
            ask=Price.from_str("2.0"),
            bid=Price.from_str("2.1"),
            bid_size=Quantity.from_int(10),
            ask_size=Quantity.from_int(10),
            ts_event=1,
            ts_init=1,
        ),
    ]

    with tempfile.TemporaryDirectory() as tempdir:
        file = os.path.join(tempdir, "test_parquet_file.parquet")

        table = objects_to_table(quotes)
        ParquetWriter()._write(table, path=file, cls=QuoteTick)

        session = PythonCatalog()
        session.add_file_with_query(
            "quotes",
            file,
            "SELECT * FROM quotes;",
            ParquetType.QuoteTick,
        )

        for chunk in session.to_query_result():
            written_quotes = list_from_capsule(chunk)
            print(written_quotes)
            # assert written_quotes == quotes
            # return
