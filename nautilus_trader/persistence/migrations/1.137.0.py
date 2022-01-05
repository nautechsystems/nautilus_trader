import pyarrow.dataset as ds

from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import write_objects
from nautilus_trader.persistence.migrate import maintain_temp_tables


FROM = "1.136.0"
TO = "1.137.0"


@maintain_temp_tables
def main(catalog: DataCatalog):
    """Rename local_symbol to native_symbol in Instruments"""
    # fs: fsspec.AbstractFileSystem = catalog.fs
    for cls in Instrument.__subclasses__():
        # Load instruments into memory
        instruments = catalog.instruments(
            as_nautilus=True,
            instrument_type=cls,
            projections={"native_symbol": ds.field("local_symbol")},
        )

        # Rewrite new instruments
        write_objects(catalog, instruments)

        # Remove
