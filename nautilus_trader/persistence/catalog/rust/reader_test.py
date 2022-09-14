import itertools
import os

import pandas as pd

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.persistence.catalog.rust.reader import ParquetReader


def test_parquet_reader_quote_ticks():
    parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_kit/data/quote_tick_data.parquet")
    reader = ParquetReader(parquet_data_path, QuoteTick)

    ticks = list(itertools.chain(*list(reader)))

    csv_data_path = os.path.join(PACKAGE_ROOT, "tests/test_kit/data/quote_tick_data.csv")
    df = pd.read_csv(csv_data_path, header=None, names="dates bid ask bid_size".split())

    assert len(ticks) == len(df)
    assert df.bid.equals(pd.Series(float(tick.bid) for tick in ticks))
    assert df.ask.equals(pd.Series(float(tick.ask) for tick in ticks))
    assert df.bid_size.equals(pd.Series(int(tick.bid_size) for tick in ticks))

    # TODO Dates are off: test data timestamps use ms instead of ns...
    # assert df.dates.equals(pd.Series([unix_nanos_to_dt(tick.ts_init).strftime("%Y%m%d %H%M%S%f") for tick in ticks]))


if __name__ == "__main__":
    test_parquet_reader_quote_ticks()
