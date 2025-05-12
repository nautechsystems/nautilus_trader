import os

from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import BarAggregation
from nautilus_trader.core.nautilus_pyo3 import BarSpecification
from nautilus_trader.core.nautilus_pyo3 import BarType
from nautilus_trader.core.nautilus_pyo3 import IndexPriceUpdate
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import MarkPriceUpdate
from nautilus_trader.core.nautilus_pyo3 import ParquetDataCatalogV2
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.core.nautilus_pyo3 import Venue
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3


AUDUSD_SIM = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))
ONE_MIN_BID = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
AUDUSD_1_MIN_BID = BarType(AUDUSD_SIM, ONE_MIN_BID)


def bar(t):
    return Bar(
        AUDUSD_1_MIN_BID,
        Price.from_str("1.00001"),
        Price.from_str("1.1"),
        Price.from_str("1.00000"),
        Price.from_str("1.00000"),
        Quantity.from_int(100_000),
        0,
        t,
    )


def test_write_2_bars_to_catalog(catalog: ParquetDataCatalog):
    # Arrange
    # Note: we use a python catalog only to setup an empty catalog every time
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_bars([bar(1), bar(2)])

    # Assert
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    assert intervals == [(1, 2)]


def test_append_data_to_catalog(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_bars([bar(1), bar(2)])
    pyo3_catalog.write_bars([bar(3)])

    # Assert
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")

    assert intervals == [(1, 2), (3, 3)]


def test_consolidate_catalog(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_bars([bar(1), bar(2)])
    pyo3_catalog.write_bars([bar(3)])
    pyo3_catalog.consolidate_catalog()

    # Assert
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    assert intervals == [(1, 3)]


def test_consolidate_catalog_with_time_range(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_bars([bar(1)])
    pyo3_catalog.write_bars([bar(2)])
    pyo3_catalog.write_bars([bar(3)])
    pyo3_catalog.consolidate_catalog(start=1, end=2)

    # Assert
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    assert intervals == [(1, 2), (3, 3)]


def test_get_missing_intervals(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_bars([bar(1), bar(2)])
    pyo3_catalog.write_bars([bar(5), bar(6)])

    # Act
    missing = pyo3_catalog.get_missing_intervals_for_request(0, 10, "bars", "AUD/USD.SIM")

    # Assert
    assert missing == [(0, 0), (3, 4), (7, 10)]


def test_reset_file_names(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_bars([bar(1), bar(2), bar(3)])

    # Manually rename the file to something incorrect
    path = os.path.join(catalog.path, "data", "bars", "AUDUSD.SIM", "1-3.parquet")
    new_path = os.path.join(catalog.path, "data", "bars", "AUDUSD.SIM", "100-200.parquet")
    os.rename(path, new_path)

    # Act
    pyo3_catalog.reset_data_file_names("bars", "AUD/USD.SIM")

    # Assert
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    assert intervals == [(1, 3)]


def test_extend_file_name(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    # Write data with a gap
    pyo3_catalog.write_bars([bar(1)])
    pyo3_catalog.write_bars([bar(4)])

    # Act - extend the first file to include the missing timestamp 2
    pyo3_catalog.extend_file_name("bars", "AUD/USD.SIM", start=2, end=3)

    # Assert
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    assert intervals == [(1, 3), (4, 4)]


def test_reset_catalog_file_names(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_bars([bar(1), bar(2)])
    pyo3_catalog.write_bars([bar(3)])

    path = os.path.join(catalog.path, "data", "bars", "AUDUSD.SIM", "1-2.parquet")
    new_path = os.path.join(catalog.path, "data", "bars", "AUDUSD.SIM", "100-200.parquet")
    os.rename(path, new_path)

    path = os.path.join(catalog.path, "data", "bars", "AUDUSD.SIM", "3-3.parquet")
    new_path = os.path.join(catalog.path, "data", "bars", "AUDUSD.SIM", "100-300.parquet")
    os.rename(path, new_path)

    # Act
    pyo3_catalog.reset_catalog_file_names()

    # Assert
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    assert intervals == [(1, 2), (3, 3)]


# Helper functions for creating test data
def quote_tick(t):
    return TestDataProviderPyo3.quote_tick(ts_init=t)


def trade_tick(t):
    return TestDataProviderPyo3.trade_tick(ts_init=t)


def order_book_delta(t):
    return TestDataProviderPyo3.order_book_delta(ts_init=t)


def order_book_depth(t):
    return TestDataProviderPyo3.order_book_depth10(ts_init=t)


def mark_price_update(t):
    instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
    return MarkPriceUpdate(
        instrument_id=instrument_id,
        value=Price.from_str("1000.00"),
        ts_event=0,
        ts_init=t,
    )


def index_price_update(t):
    instrument_id = TestIdProviderPyo3.ethusdt_binance_id()
    return IndexPriceUpdate(
        instrument_id=instrument_id,
        value=Price.from_str("1000.00"),
        ts_event=0,
        ts_init=t,
    )


def test_write_quote_ticks(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_quote_ticks([quote_tick(1), quote_tick(2)])

    # Assert
    # Check that files were created
    used_files = pyo3_catalog.query_files("quotes", ["ETH/USDT.BINANCE"])
    assert len(used_files) >= 1


def test_write_trade_ticks(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_trade_ticks([trade_tick(1), trade_tick(2)])

    # Assert
    # Check that files were created
    used_files = pyo3_catalog.query_files("trades", ["ETH/USDT.BINANCE"])
    assert len(used_files) >= 1


def test_write_order_book_deltas(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_order_book_deltas([order_book_delta(1), order_book_delta(2)])

    # Assert
    # Check that files were created
    used_files = pyo3_catalog.query_files("order_book_deltas", ["ETH/USDT.BINANCE"])
    assert len(used_files) >= 1


def test_write_mark_price_updates(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_mark_price_updates([mark_price_update(1), mark_price_update(2)])

    # Assert
    # Check that files were created
    used_files = pyo3_catalog.query_files("mark_prices", ["ETH/USDT.BINANCE"])
    assert len(used_files) >= 1


def test_write_index_price_updates(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    pyo3_catalog.write_index_price_updates([index_price_update(1), index_price_update(2)])

    # Assert
    # Check that files were created
    used_files = pyo3_catalog.query_files("index_prices", ["ETH/USDT.BINANCE"])
    assert len(used_files) >= 1


def test_query_files(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_bars([bar(1), bar(2)])
    pyo3_catalog.write_bars([bar(3), bar(4)])

    # Act
    files = pyo3_catalog.query_files("bars", ["AUD/USD.SIM"])

    # Assert
    assert len(files) == 2


def test_query_files_with_multiple_files(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_bars([bar(1), bar(2)])
    pyo3_catalog.write_bars([bar(3), bar(4)])
    pyo3_catalog.write_bars([bar(5), bar(6)])

    # Act
    files = pyo3_catalog.query_files("bars", ["AUD/USD.SIM"])

    # Assert
    assert len(files) == 3


def test_get_intervals_empty(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")

    # Assert
    assert len(intervals) == 0
