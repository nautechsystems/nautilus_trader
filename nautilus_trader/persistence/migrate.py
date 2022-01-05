from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import write_objects


def create_temp_table(func):
    """Make a temporary copy of any parquet dataset class called by `write_tables`"""

    def inner(*args, **kwargs):
        try:
            return func(*args, **kwargs)
        except Exception:
            # Restore old table
            print()

    return inner


write_objects = create_temp_table(write_objects)


def migrate(catalog: DataCatalog, version_from: str, version_to: str):
    """Migrate the `catalog` between versions `version_from` and `version_to`"""
    pass
