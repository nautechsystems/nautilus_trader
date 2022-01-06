import warnings

import fsspec
import pyarrow.dataset as ds
from tqdm import tqdm

from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import write_objects


FROM = "1.134.0"
TO = "1.135.0"

# EXAMPLE ONLY - not working


def main(catalog: DataCatalog):
    """Rename match_id to trade_id in TradeTick"""
    fs: fsspec.AbstractFileSystem = catalog.fs

    print("Loading instrument ids")
    instrument_ids = catalog.query(TradeTick, table_kwargs={"columns": ["instrument_id"]})[
        "instrument_id"
    ].unique()

    tmp_catalog = DataCatalog(str(catalog.path) + "_tmp")
    tmp_catalog.fs = catalog.fs

    for ins_id in tqdm(instrument_ids):

        # Load trades for instrument
        trades = catalog.trade_ticks(
            instrument_ids=[ins_id],
            projections={"trade_id": ds.field("match_id")},
            as_nautilus=True,
        )

        # Create temp parquet in case of error
        fs.move(
            f"{catalog.path}/data/trade_tick.parquet/instrument_id={ins_id}",
            f"{catalog.path}/data/trade_tick.parquet_tmp/instrument_id={ins_id}",
            recursive=True,
        )

        try:
            # Rewrite to new catalog
            write_objects(tmp_catalog, trades)

            # Ensure we can query again
            _ = tmp_catalog.trade_ticks(instrument_ids=[ins_id], as_nautilus=True)

            # Clear temp parquet
            fs.rm(
                f"{catalog.path}/data/trade_tick.parquet_tmp/instrument_id={ins_id}", recursive=True
            )

        except Exception:
            warnings.warn(f"Failed to write or read instrument_id {ins_id}")
            fs.move(
                f"{catalog.path}/data/trade_tick.parquet_tmp/instrument_id={ins_id}",
                f"{catalog.path}/data/trade_tick.parquet/instrument_id={ins_id}",
                recursive=True,
            )
