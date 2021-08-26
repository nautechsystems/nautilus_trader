import pytest

from nautilus_trader.persistence.catalog import DataCatalog


@pytest.fixture(scope="function")
def catalog():
    return DataCatalog.from_env()


@pytest.fixture(scope="function")
def catalog_data():
    pass
