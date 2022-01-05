from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import write_objects


def create_temp_table(func):
    def inner(*args, **kwargs):
        return func(*args, **kwargs)

    return inner


write_objects = create_temp_table(write_objects)


def maintain_temp_tables(func):
    def inner(*args, **kwargs):
        # Create temp tables
        try:
            return func(*args, **kwargs)
        except Exception:
            # Error - restore temp tables
            print()

    return inner


def migrate(catalog: DataCatalog, version_from: str, version_to: str):
    pass
