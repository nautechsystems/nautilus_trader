import pytest

from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import BarAggregation
from nautilus_trader.core.nautilus_pyo3 import BarSpecification
from nautilus_trader.core.nautilus_pyo3 import BarType
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import ParquetDataCatalogV2
from nautilus_trader.core.nautilus_pyo3 import ParquetWriteMode
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.core.nautilus_pyo3 import Venue
from nautilus_trader.persistence.catalog import ParquetDataCatalog


AUDUSD_SIM = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
ONE_MIN_BID = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
AUDUSD_1_MIN_BID = BarType(AUDUSD_SIM, ONE_MIN_BID)

bar1 = Bar(
    AUDUSD_1_MIN_BID,
    Price.from_str("1.00001"),
    Price.from_str("1.1"),
    Price.from_str("1.00000"),
    Price.from_str("1.00000"),
    Quantity.from_int(100_000),
    0,
    1,
)

bar2 = Bar(
    AUDUSD_1_MIN_BID,
    Price.from_str("1.00001"),
    Price.from_str("1.1"),
    Price.from_str("1.00000"),
    Price.from_str("1.00000"),
    Quantity.from_int(100_000),
    0,
    2,
)

bar3 = Bar(
    AUDUSD_1_MIN_BID,
    Price.from_str("1.00001"),
    Price.from_str("1.1"),
    Price.from_str("1.00000"),
    Price.from_str("1.00000"),
    Quantity.from_int(100_000),
    0,
    3,
)


def test_write_2_bars_to_catalog(catalog: ParquetDataCatalog):
    # Arrange
    # Note: we use a python catalog only to setup an empty catalog every time
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_bars([bar1, bar2])

    # Assert
    first_timestamp = last_timestamp = pyo3_catalog.query_timestamp_bound(
        "bars",
        "AUD/USD.SIM",
        False,
    )
    last_timestamp = pyo3_catalog.query_timestamp_bound("bars", "AUD/USD.SIM", True)
    assert first_timestamp == 1
    assert last_timestamp == 2

    used_files = pyo3_catalog.query_parquet_files("bars", "AUD/USD.SIM")
    assert len(used_files) == 1


def test_append_data_to_catalog(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_bars([bar1, bar2])
    pyo3_catalog.write_bars([bar3], ParquetWriteMode.APPEND)

    # Assert
    first_timestamp = last_timestamp = pyo3_catalog.query_timestamp_bound(
        "bars",
        "AUD/USD.SIM",
        False,
    )
    last_timestamp = pyo3_catalog.query_timestamp_bound("bars", "AUD/USD.SIM", True)
    assert first_timestamp == 1
    assert last_timestamp == 3

    used_files = pyo3_catalog.query_parquet_files("bars", "AUD/USD.SIM")
    assert len(used_files) == 1


def test_prepend_data_to_catalog(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_bars([bar2, bar3])
    pyo3_catalog.write_bars([bar1], ParquetWriteMode.PREPEND)

    # Assert
    first_timestamp = last_timestamp = pyo3_catalog.query_timestamp_bound(
        "bars",
        "AUD/USD.SIM",
        False,
    )
    last_timestamp = pyo3_catalog.query_timestamp_bound("bars", "AUD/USD.SIM", True)
    assert first_timestamp == 1
    assert last_timestamp == 3

    used_files = pyo3_catalog.query_parquet_files("bars", "AUD/USD.SIM")
    assert len(used_files) == 1


def test_write_3_bars_to_catalog_with_new_file(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_bars([bar1, bar2])
    pyo3_catalog.write_bars([bar3], ParquetWriteMode.NEWFILE)

    # Assert
    first_timestamp = last_timestamp = pyo3_catalog.query_timestamp_bound(
        "bars",
        "AUD/USD.SIM",
        False,
    )
    last_timestamp = pyo3_catalog.query_timestamp_bound("bars", "AUD/USD.SIM", True)
    assert first_timestamp == 1
    assert last_timestamp == 3

    used_files = pyo3_catalog.query_parquet_files("bars", "AUD/USD.SIM")
    assert len(used_files) == 2


def test_consolidate_catalog(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_bars([bar1, bar2])
    pyo3_catalog.write_bars([bar3], ParquetWriteMode.NEWFILE)
    pyo3_catalog.consolidate_catalog()

    # Assert
    first_timestamp = last_timestamp = pyo3_catalog.query_timestamp_bound(
        "bars",
        "AUD/USD.SIM",
        False,
    )
    last_timestamp = pyo3_catalog.query_timestamp_bound("bars", "AUD/USD.SIM", True)
    assert first_timestamp == 1
    assert last_timestamp == 3

    used_files = pyo3_catalog.query_parquet_files("bars", "AUD/USD.SIM")
    assert len(used_files) == 1


def test_consolidate_catalog_with_intersection(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_bars([bar1, bar2])
    pyo3_catalog.write_bars([bar2], ParquetWriteMode.NEWFILE)

    # Assert
    with pytest.raises(OSError):
        pyo3_catalog.consolidate_catalog()


def test_consolidate_data(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_bars([bar1, bar2])
    pyo3_catalog.write_bars([bar3], ParquetWriteMode.NEWFILE)
    pyo3_catalog.consolidate_data("bars", "AUD/USD.SIM")

    # Assert
    first_timestamp = last_timestamp = pyo3_catalog.query_timestamp_bound(
        "bars",
        "AUD/USD.SIM",
        False,
    )
    last_timestamp = pyo3_catalog.query_timestamp_bound("bars", "AUD/USD.SIM", True)
    assert first_timestamp == 1
    assert last_timestamp == 3

    used_files = pyo3_catalog.query_parquet_files("bars", "AUD/USD.SIM")
    assert len(used_files) == 1
