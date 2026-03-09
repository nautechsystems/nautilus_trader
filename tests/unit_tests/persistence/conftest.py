import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.betfair.parsing.core import betting_instruments_from_file
from nautilus_trader.adapters.betfair.parsing.core import parse_betfair_file
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.test_kit.mocks.data import setup_catalog


@pytest.fixture(name="catalog_memory")
def fixture_catalog_memory(tmp_path) -> ParquetDataCatalog:
    return setup_catalog(protocol="memory", path=tmp_path / "catalog_memory")


@pytest.fixture(name="catalog")
def fixture_catalog(tmp_path) -> ParquetDataCatalog:
    return setup_catalog(protocol="file", path=tmp_path / "catalog_file")


@pytest.fixture(name="catalog_betfair")
def fixture_catalog_betfair(catalog: ParquetDataCatalog) -> ParquetDataCatalog:
    filename = TEST_DATA_DIR / "betfair" / "1-166564490.bz2"

    # Write betting instruments
    instruments = betting_instruments_from_file(
        filename,
        currency="GBP",
        ts_event=0,
        ts_init=0,
        min_notional=Money(1, GBP),
    )
    catalog.write_data(instruments)

    # Write data
    data = list(
        parse_betfair_file(
            filename,
            currency="GBP",
            min_notional=Money(1, GBP),
        ),
    )
    catalog.write_data(data)

    return catalog
