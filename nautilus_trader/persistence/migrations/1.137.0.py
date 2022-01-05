import warnings

import fsspec
import pyarrow.dataset as ds

from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import write_objects


FROM = "1.136.0"
TO = "1.137.0"


def main(catalog: DataCatalog):
    """Rename local_symbol to native_symbol in Instruments"""
    fs: fsspec.AbstractFileSystem = catalog.fs
    for cls in Instrument.__subclasses__():
        # Load instruments into memory
        instruments = catalog.instruments(
            as_nautilus=True,
            instrument_type=cls,
            projections={"native_symbol": ds.field("local_symbol")},
        )

        # Create temp parquet in case of error
        fs.move(
            f"{catalog.path}/data/equity.parquet",
            f"{catalog.path}/data/equity.parquet_tmp",
            recursive=True,
        )

        try:
            # Rewrite new instruments
            write_objects(catalog, instruments)

            # Ensure we can query again
            _ = catalog.instruments(instrument_type=cls, as_nautilus=True)

            # Clear temp parquet
            fs.rm(f"{catalog.path}/data/equity.parquet_tmp", recursive=True)
        except Exception:
            warnings.warn(f"Failed to write or read instrument type {cls}")
            fs.move(
                f"{catalog.path}/data/equity.parquet_tmp",
                f"{catalog.path}/data/equity.parquet",
                recursive=True,
            )
