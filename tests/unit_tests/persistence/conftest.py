import pytest

from nautilus_trader.adapters.betfair.parsing.core import betting_instruments_from_file
from nautilus_trader.adapters.betfair.parsing.core import parse_betfair_file
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from tests import TEST_DATA_DIR


@pytest.fixture
def memory_data_catalog():
    return data_catalog_setup(protocol="memory")


@pytest.fixture
def data_catalog():
    return data_catalog_setup(protocol="file")


@pytest.fixture
def betfair_catalog(data_catalog):
    fn = TEST_DATA_DIR + "/betfair/1.166564490.bz2"

    # Write betting instruments
    instruments = betting_instruments_from_file(fn)
    data_catalog.write_data(instruments)

    # Write data
    data = list(parse_betfair_file(fn))
    data_catalog.write_data(data)

    return data_catalog
