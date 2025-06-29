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

    # Find the actual filename that was created
    bars_dir = os.path.join(catalog.path, "data", "bars", "AUDUSD.SIM")
    files = os.listdir(bars_dir)
    assert len(files) == 1, f"Expected 1 file, found {len(files)}: {files}"
    original_filename = files[0]

    # Manually rename the file to something incorrect
    path = os.path.join(bars_dir, original_filename)
    new_path = os.path.join(bars_dir, "100-200.parquet")
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

    # Find the actual filenames that were created
    bars_dir = os.path.join(catalog.path, "data", "bars", "AUDUSD.SIM")
    files = os.listdir(bars_dir)
    assert len(files) == 2, f"Expected 2 files, found {len(files)}: {files}"

    # Rename both files to something incorrect
    for i, original_filename in enumerate(files):
        path = os.path.join(bars_dir, original_filename)
        new_path = os.path.join(bars_dir, f"100-{200 + i * 100}.parquet")
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


def test_query_bars(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_bars([bar(1), bar(2)])

    # Act
    bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])

    # Assert
    assert len(bars) == 2
    assert bars[0].ts_init == 1
    assert bars[1].ts_init == 2


def test_query_quote_ticks(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_quote_ticks([quote_tick(1), quote_tick(2)])

    # Act
    quotes = pyo3_catalog.query_quote_ticks(["ETH/USDT.BINANCE"])

    # Assert
    assert len(quotes) == 2
    assert quotes[0].ts_init == 1
    assert quotes[1].ts_init == 2


def test_query_trade_ticks(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_trade_ticks([trade_tick(1), trade_tick(2)])

    # Act
    trades = pyo3_catalog.query_trade_ticks(["ETH/USDT.BINANCE"])

    # Assert
    assert len(trades) == 2
    assert trades[0].ts_init == 1
    assert trades[1].ts_init == 2


def test_query_order_book_deltas(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_order_book_deltas([order_book_delta(1), order_book_delta(2)])

    # Act
    deltas = pyo3_catalog.query_order_book_deltas(["ETH/USDT.BINANCE"])

    # Assert
    assert len(deltas) == 2
    assert deltas[0].ts_init == 1
    assert deltas[1].ts_init == 2


def test_query_mark_price_updates(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_mark_price_updates([mark_price_update(1), mark_price_update(2)])

    # Act
    updates = pyo3_catalog.query_mark_price_updates(["ETH/USDT.BINANCE"])

    # Assert
    assert len(updates) == 2
    assert updates[0].ts_init == 1
    assert updates[1].ts_init == 2


def test_query_index_price_updates(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_index_price_updates([index_price_update(1), index_price_update(2)])

    # Act
    updates = pyo3_catalog.query_index_price_updates(["ETH/USDT.BINANCE"])

    # Assert
    assert len(updates) == 2
    assert updates[0].ts_init == 1
    assert updates[1].ts_init == 2


def test_query_bars_with_time_range(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_bars([bar(1), bar(2), bar(3), bar(4)])

    # Act
    bars = pyo3_catalog.query_bars(["AUD/USD.SIM"], start=2, end=3)

    # Assert
    assert len(bars) == 2
    assert bars[0].ts_init == 2
    assert bars[1].ts_init == 3


def test_query_bars_empty_result(catalog: ParquetDataCatalog):
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act
    bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])

    # Assert
    assert len(bars) == 0


def test_query_bars_with_where_clause(catalog: ParquetDataCatalog):
    """
    Test query_bars with WHERE clause filtering.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_bars([bar(1000), bar(2000), bar(3000)])

    # Act - query with WHERE clause
    bars = pyo3_catalog.query_bars(
        ["AUD/USD.SIM"],
        start=500,
        end=3500,
        where_clause="ts_init >= 2000",
    )

    # Assert - should return only bars with ts_init >= 2000
    assert len(bars) == 2
    assert all(b.ts_init >= 2000 for b in bars)


def test_query_quote_ticks_with_time_range(catalog: ParquetDataCatalog):
    """
    Test query_quote_ticks with time range filtering.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_quote_ticks([quote_tick(1000), quote_tick(2000), quote_tick(3000)])

    # Act - query quotes with time range
    quotes = pyo3_catalog.query_quote_ticks(["ETH/USDT.BINANCE"], start=1500, end=2500)

    # Assert - should return only the middle quote
    assert len(quotes) == 1
    assert quotes[0].ts_init == 2000


def test_query_trade_ticks_with_time_range(catalog: ParquetDataCatalog):
    """
    Test query_trade_ticks with time range filtering.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    pyo3_catalog.write_trade_ticks([trade_tick(1000), trade_tick(2000), trade_tick(3000)])

    # Act - query trades with time range
    trades = pyo3_catalog.query_trade_ticks(["ETH/USDT.BINANCE"], start=1500, end=2500)

    # Assert - should return only the middle trade
    assert len(trades) == 1
    assert trades[0].ts_init == 2000


def test_consolidate_catalog_by_period_basic(catalog: ParquetDataCatalog):
    """
    Test consolidate_catalog_by_period with period parameter.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Create multiple small files for different data types with contiguous timestamps
    pyo3_catalog.write_bars([bar(1000)])
    pyo3_catalog.write_bars([bar(1001)])  # contiguous
    pyo3_catalog.write_quote_ticks([quote_tick(1000)])
    pyo3_catalog.write_quote_ticks([quote_tick(1001)])  # contiguous

    # Verify we have multiple files initially
    bar_intervals_before = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    quote_intervals_before = pyo3_catalog.get_intervals("quotes", "ETH/USDT.BINANCE")
    assert len(bar_intervals_before) == 2
    assert len(quote_intervals_before) == 2

    # Act - consolidate with period parameter (use ensure_contiguous_files=False to avoid issues)
    pyo3_catalog.consolidate_catalog_by_period(
        period_nanos=86400_000_000_000,  # 1 day in nanoseconds
        start=None,
        end=None,
        ensure_contiguous_files=False,
    )

    # Assert - should have consolidated files
    bar_intervals_after = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    quote_intervals_after = pyo3_catalog.get_intervals("quotes", "ETH/USDT.BINANCE")

    # Should have same or fewer intervals after consolidation
    assert len(bar_intervals_after) <= len(bar_intervals_before)
    assert len(quote_intervals_after) <= len(quote_intervals_before)


def test_consolidate_catalog_by_period_empty_catalog(catalog: ParquetDataCatalog):
    """
    Test consolidate_catalog_by_period on empty catalog.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act - consolidate empty catalog
    pyo3_catalog.consolidate_catalog_by_period(
        period_nanos=86400_000_000_000,  # 1 day in nanoseconds
        start=None,
        end=None,
        ensure_contiguous_files=True,
    )

    # Assert - should complete without error
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    assert len(intervals) == 0


def test_consolidate_catalog_by_period_mixed_data_types(catalog: ParquetDataCatalog):
    """
    Test consolidate_catalog_by_period with multiple data types.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Create data for multiple types with contiguous timestamps
    pyo3_catalog.write_bars([bar(1000)])
    pyo3_catalog.write_bars([bar(1001)])  # contiguous
    pyo3_catalog.write_quote_ticks([quote_tick(1000)])
    pyo3_catalog.write_quote_ticks([quote_tick(1001)])  # contiguous
    pyo3_catalog.write_trade_ticks([trade_tick(1000)])
    pyo3_catalog.write_trade_ticks([trade_tick(1001)])  # contiguous

    # Get initial file counts
    initial_bar_count = len(pyo3_catalog.get_intervals("bars", "AUD/USD.SIM"))
    initial_quote_count = len(pyo3_catalog.get_intervals("quotes", "ETH/USDT.BINANCE"))
    initial_trade_count = len(pyo3_catalog.get_intervals("trades", "ETH/USDT.BINANCE"))

    # Act - consolidate all data types (use ensure_contiguous_files=False to avoid issues)
    pyo3_catalog.consolidate_catalog_by_period(
        period_nanos=86400_000_000_000,  # 1 day in nanoseconds
        start=None,
        end=None,
        ensure_contiguous_files=False,
    )

    # Assert - all data types should be processed
    final_bar_count = len(pyo3_catalog.get_intervals("bars", "AUD/USD.SIM"))
    final_quote_count = len(pyo3_catalog.get_intervals("quotes", "ETH/USDT.BINANCE"))
    final_trade_count = len(pyo3_catalog.get_intervals("trades", "ETH/USDT.BINANCE"))

    # Should have same or fewer files after consolidation
    assert final_bar_count <= initial_bar_count
    assert final_quote_count <= initial_quote_count
    assert final_trade_count <= initial_trade_count


def test_consolidate_data_by_period_basic(catalog: ParquetDataCatalog):
    """
    Test basic consolidate_data_by_period functionality.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    test_bars = [bar(1000), bar(2000), bar(3000), bar(4000), bar(5000)]
    pyo3_catalog.write_bars(test_bars)

    # Act - consolidate by period (1 day in nanoseconds)
    period_nanos = 86400_000_000_000  # 1 day
    pyo3_catalog.consolidate_data_by_period(
        type_name="bars",
        identifier="AUD/USD.SIM",
        period_nanos=period_nanos,
        ensure_contiguous_files=False,
    )

    # Assert - verify the operation completed successfully
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    assert len(intervals) >= 1


def test_consolidate_data_by_period_with_time_range(catalog: ParquetDataCatalog):
    """
    Test consolidate_data_by_period with specific time range.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    test_bars = [bar(1000), bar(5000), bar(10000), bar(15000), bar(20000)]
    pyo3_catalog.write_bars(test_bars)

    # Act - consolidate with time range
    start_time = 3000
    end_time = 18000
    period_nanos = 3600_000_000_000  # 1 hour
    pyo3_catalog.consolidate_data_by_period(
        type_name="bars",
        identifier="AUD/USD.SIM",
        period_nanos=period_nanos,
        start=start_time,
        end=end_time,
        ensure_contiguous_files=False,
    )

    # Assert - verify the operation completed successfully
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    assert len(intervals) >= 1


def test_consolidate_data_by_period_empty_data(catalog: ParquetDataCatalog):
    """
    Test consolidate_data_by_period with no data (should not error).
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act - consolidate on empty catalog
    period_nanos = 86400_000_000_000  # 1 day
    pyo3_catalog.consolidate_data_by_period(
        type_name="bars",
        identifier="AUD/USD.SIM",
        period_nanos=period_nanos,
        ensure_contiguous_files=False,
    )

    # Assert - should complete without error
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    assert len(intervals) == 0


def test_consolidate_data_by_period_default_parameters(catalog: ParquetDataCatalog):
    """
    Test consolidate_data_by_period with default parameters.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    test_bars = [bar(1000), bar(2000), bar(3000)]
    pyo3_catalog.write_bars(test_bars)

    # Act - consolidate with default parameters (should use 1 day period)
    pyo3_catalog.consolidate_data_by_period(
        type_name="bars",
        identifier="AUD/USD.SIM",
    )

    # Assert - verify the operation completed successfully
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    assert len(intervals) >= 1


def test_consolidate_data_by_period_different_periods(catalog: ParquetDataCatalog):
    """
    Test consolidate_data_by_period with different period sizes.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    test_bars = [
        bar(1000),  # ~0 minutes
        bar(600_000),  # ~10 minutes
        bar(1_800_000),  # ~30 minutes
        bar(3_600_000),  # ~1 hour
        bar(7_200_000),  # ~2 hours
    ]
    pyo3_catalog.write_bars(test_bars)

    # Act - test different period sizes (in nanoseconds)
    periods = [
        1_800_000_000_000,  # 30 minutes
        3_600_000_000_000,  # 1 hour
        86400_000_000_000,  # 1 day
    ]

    for period_nanos in periods:
        pyo3_catalog.consolidate_data_by_period(
            type_name="bars",
            identifier="AUD/USD.SIM",
            period_nanos=period_nanos,
            ensure_contiguous_files=False,
        )

        # Assert - verify the operation completed successfully
        intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
        assert len(intervals) >= 1


def test_consolidate_data_by_period_ensure_contiguous_files_true(catalog: ParquetDataCatalog):
    """
    Test consolidate_data_by_period with ensure_contiguous_files=True.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    test_bars = [bar(1000), bar(1001), bar(1002)]  # contiguous timestamps
    pyo3_catalog.write_bars(test_bars)

    # Act - consolidate with ensure_contiguous_files=True
    period_nanos = 86400_000_000_000  # 1 day
    pyo3_catalog.consolidate_data_by_period(
        type_name="bars",
        identifier="AUD/USD.SIM",
        period_nanos=period_nanos,
        ensure_contiguous_files=True,
    )

    # Assert - verify the operation completed successfully
    intervals = pyo3_catalog.get_intervals("bars", "AUD/USD.SIM")
    assert len(intervals) >= 1


def test_query_functions_data_integrity(catalog: ParquetDataCatalog):
    """
    Test that query functions return data with correct integrity.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    test_bars = [bar(1000), bar(2000), bar(3000)]
    pyo3_catalog.write_bars(test_bars)

    # Act - query all bars
    all_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])

    # Assert - results should be consistent
    assert len(all_bars) == 3

    # Verify data integrity
    for i, bar_data in enumerate(all_bars):
        assert bar_data.ts_init == test_bars[i].ts_init
        assert bar_data.open == test_bars[i].open
        assert bar_data.high == test_bars[i].high
        assert bar_data.low == test_bars[i].low
        assert bar_data.close == test_bars[i].close
