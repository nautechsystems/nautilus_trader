from collections import defaultdict
import os
import pathlib

import pandas as pd
import pyarrow as pa
import pyarrow.parquet as pq

from nautilus_trader.backtest.storage.parsing import dictionary_columns
from nautilus_trader.backtest.storage.parsing import nautilus_to_dict
from nautilus_trader.model.orderbook.book import OrderBookDelta
from nautilus_trader.model.orderbook.book import OrderBookDeltas
from nautilus_trader.model.orderbook.book import OrderBookSnapshot


def nautilus_to_parquet(objects):
    path = pathlib.Path(os.environ["NAUTILUS_BACKTEST_DIR"])
    type_conv = {OrderBookDeltas: OrderBookDelta, OrderBookSnapshot: OrderBookDelta}
    tables = defaultdict(list)
    for obj in objects:
        cls = type_conv.get(type(obj), type(obj))
        tables[cls].extend(nautilus_to_dict(obj))

    # Create directory if not exists
    assert not path.exists() or path.is_dir()
    path.mkdir(exist_ok=True)

    for cls in tables:
        df = pd.DataFrame(tables[cls])

        # Load any existing data, drop dupes
        fn = path.joinpath(f"{cls.__name__.lower()}.parquet")
        if os.path.exists(fn):
            existing = pd.read_parquet(fn)
            df = df.append(existing).drop_duplicates()

        # Write
        df = df.astype({k: "category" for k in dictionary_columns.get(cls, [])})
        pq.write_table(pa.Table.from_pandas(df), fn)
    return tables
