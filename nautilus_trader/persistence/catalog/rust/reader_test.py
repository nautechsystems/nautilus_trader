import itertools
import os

import pandas as pd

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.persistence.catalog.rust.reader import ParquetFileReader
from nautilus_trader.persistence.catalog.rust.writer import ParquetWriter


def test_parquet_writer():
    from nautilus_trader.backtest.data.providers import TestInstrumentProvider
    from nautilus_trader.backtest.data.wranglers import QuoteTickDataWrangler

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
    # writer.write(quotes) # TODO - breaks
    writer.write(quotes[:5000])
    writer.write(quotes[5000:])
    data = writer.drop()

    with open(file_path, "wb") as f:
        f.write(data)

    # Ensure we're reading the same ticks back
    reader = ParquetFileReader(QuoteTick, file_path)
    ticks = list(itertools.chain(*list(reader)))
    assert len(ticks) == len(quotes)
    assert ticks[0] == quotes[0]
    assert ticks[-1] == quotes[-1]

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


if __name__ == "__main__":
    test_parquet_writer()
    test_parquet_reader_quote_ticks()
