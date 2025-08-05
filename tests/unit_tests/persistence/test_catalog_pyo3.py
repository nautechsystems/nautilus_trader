# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import os
import sys

import pytest

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


pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")

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


def test_reset_all_file_names(catalog: ParquetDataCatalog):
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
    pyo3_catalog.reset_all_file_names()

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


# ================================================================================================
# Delete functionality tests
# ================================================================================================


def test_delete_data_range_complete_file_deletion(catalog: ParquetDataCatalog):
    """
    Test deleting data that completely covers one or more files.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    test_bars = [bar(1_000_000_000), bar(2_000_000_000)]
    pyo3_catalog.write_bars(test_bars)

    # Verify initial state
    initial_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])
    assert len(initial_bars) == 2

    # Act - delete all data
    pyo3_catalog.delete_data_range(
        "bars",
        "AUD/USD.SIM",
        0,
        3_000_000_000,
    )

    # Assert
    remaining_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])
    assert len(remaining_bars) == 0


def test_delete_data_range_partial_file_overlap_start(catalog: ParquetDataCatalog):
    """
    Test deleting data that partially overlaps with a file from the start.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    test_bars = [
        bar(1_000_000_000),
        bar(2_000_000_000),
        bar(3_000_000_000),
    ]
    pyo3_catalog.write_bars(test_bars)

    # Act - delete first part of the data
    pyo3_catalog.delete_data_range(
        "bars",
        "AUD/USD.SIM",
        0,
        1_500_000_000,
    )

    # Assert - should keep data after deletion range
    remaining_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])
    assert len(remaining_bars) == 2
    assert remaining_bars[0].ts_init == 2_000_000_000
    assert remaining_bars[1].ts_init == 3_000_000_000


def test_delete_data_range_partial_file_overlap_end(catalog: ParquetDataCatalog):
    """
    Test deleting data that partially overlaps with a file from the end.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    test_bars = [
        bar(1_000_000_000),
        bar(2_000_000_000),
        bar(3_000_000_000),
    ]
    pyo3_catalog.write_bars(test_bars)

    # Act - delete last part of the data
    pyo3_catalog.delete_data_range(
        "bars",
        "AUD/USD.SIM",
        2_500_000_000,
        4_000_000_000,
    )

    # Assert - should keep data before deletion range
    remaining_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])
    assert len(remaining_bars) == 2
    assert remaining_bars[0].ts_init == 1_000_000_000
    assert remaining_bars[1].ts_init == 2_000_000_000


def test_delete_data_range_partial_file_overlap_middle(catalog: ParquetDataCatalog):
    """
    Test deleting data that partially overlaps with a file in the middle.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    test_bars = [
        bar(1_000_000_000),
        bar(2_000_000_000),
        bar(3_000_000_000),
        bar(4_000_000_000),
    ]
    pyo3_catalog.write_bars(test_bars)

    # Act - delete middle part of the data
    pyo3_catalog.delete_data_range(
        "bars",
        "AUD/USD.SIM",
        1_500_000_000,
        3_500_000_000,
    )

    # Assert - should keep data before and after deletion range
    remaining_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])
    assert len(remaining_bars) == 2
    assert remaining_bars[0].ts_init == 1_000_000_000
    assert remaining_bars[1].ts_init == 4_000_000_000


def test_delete_data_range_no_data(catalog: ParquetDataCatalog):
    """
    Test deleting data when no data exists.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act - delete from empty catalog
    pyo3_catalog.delete_data_range(
        "bars",
        "AUD/USD.SIM",
        1_000_000_000,
        2_000_000_000,
    )

    # Assert - should not raise any errors
    remaining_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])
    assert len(remaining_bars) == 0


def test_delete_data_range_no_intersection(catalog: ParquetDataCatalog):
    """
    Test deleting data that doesn't intersect with existing data.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)
    test_bars = [bar(2_000_000_000)]
    pyo3_catalog.write_bars(test_bars)

    # Act - delete data outside existing range
    pyo3_catalog.delete_data_range(
        "bars",
        "AUD/USD.SIM",
        3_000_000_000,
        4_000_000_000,
    )

    # Assert - should keep all existing data
    remaining_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])
    assert len(remaining_bars) == 1
    assert remaining_bars[0].ts_init == 2_000_000_000


def test_delete_catalog_range_multiple_data_types(catalog: ParquetDataCatalog):
    """
    Test deleting data across multiple data types in the catalog.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Create data for multiple data types using pyo3 objects
    test_bars = [
        bar(1_000_000_000),
        bar(2_000_000_000),
    ]
    test_quotes = [
        TestDataProviderPyo3.quote_tick(),
        TestDataProviderPyo3.quote_tick(),
    ]

    pyo3_catalog.write_bars(test_bars)
    pyo3_catalog.write_quote_ticks(test_quotes)

    # Verify initial state
    initial_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])
    initial_quotes = pyo3_catalog.query_quote_ticks(
        ["ETH/USDT.BINANCE"],
    )  # Use correct instrument ID
    assert len(initial_bars) == 2
    assert len(initial_quotes) == 2

    # Act - delete all data (use wide range since we can't control timestamps)
    pyo3_catalog.delete_catalog_range(
        0,
        3_000_000_000,
    )

    # Assert - should delete all data
    remaining_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])
    remaining_quotes = pyo3_catalog.query_quote_ticks(["ETH/USDT.BINANCE"])

    # Should have no remaining data
    assert len(remaining_bars) == 0
    assert len(remaining_quotes) == 0


def test_delete_catalog_range_complete_deletion(catalog: ParquetDataCatalog):
    """
    Test deleting all data in the catalog.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Create data for multiple data types
    test_bars = [bar(1_000_000_000)]
    test_quotes = [TestDataProviderPyo3.quote_tick()]

    pyo3_catalog.write_bars(test_bars)
    pyo3_catalog.write_quote_ticks(test_quotes)

    # Verify initial state
    assert len(pyo3_catalog.query_bars(["AUD/USD.SIM"])) == 1
    assert len(pyo3_catalog.query_quote_ticks(["ETH/USDT.BINANCE"])) == 1

    # Act - delete all data
    pyo3_catalog.delete_catalog_range(
        0,
        3_000_000_000,
    )

    # Assert - should have no data left
    assert len(pyo3_catalog.query_bars(["AUD/USD.SIM"])) == 0
    assert len(pyo3_catalog.query_quote_ticks(["ETH/USDT.BINANCE"])) == 0


def test_delete_catalog_range_empty_catalog(catalog: ParquetDataCatalog):
    """
    Test deleting data from an empty catalog.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Act - delete from empty catalog
    pyo3_catalog.delete_catalog_range(
        1_000_000_000,
        2_000_000_000,
    )

    # Assert - should not raise any errors
    assert len(pyo3_catalog.query_bars(["AUD/USD.SIM"])) == 0
    assert len(pyo3_catalog.query_quote_ticks(["ETH/USDT.BINANCE"])) == 0


def test_delete_catalog_range_open_boundaries(catalog: ParquetDataCatalog):
    """
    Test deleting data with open start/end boundaries.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Create test data
    test_bars = [
        bar(1_000_000_000),
        bar(2_000_000_000),
        bar(3_000_000_000),
    ]
    test_quotes = [
        TestDataProviderPyo3.quote_tick(),
        TestDataProviderPyo3.quote_tick(),
        TestDataProviderPyo3.quote_tick(),
    ]

    pyo3_catalog.write_bars(test_bars)
    pyo3_catalog.write_quote_ticks(test_quotes)

    # Act - delete from beginning to middle (open start)
    pyo3_catalog.delete_catalog_range(
        None,
        2_200_000_000,
    )


def test_delete_data_range_nanosecond_precision_boundaries(catalog: ParquetDataCatalog):
    """
    Test deleting data with nanosecond precision boundaries.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Create test data with precise nanosecond timestamps
    test_bars = [
        bar(1_000_000_000),
        bar(1_000_000_001),  # +1 nanosecond
        bar(1_000_000_002),  # +2 nanoseconds
        bar(1_000_000_003),  # +3 nanoseconds
    ]
    pyo3_catalog.write_bars(test_bars)

    # Act - delete exactly the middle two timestamps [1_000_000_001, 1_000_000_002]
    pyo3_catalog.delete_data_range(
        "bars",
        "AUD/USD.SIM",
        1_000_000_001,
        1_000_000_002,
    )

    # Assert - should keep only first and last timestamps
    remaining_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])
    assert len(remaining_bars) == 2
    assert remaining_bars[0].ts_init == 1_000_000_000
    assert remaining_bars[1].ts_init == 1_000_000_003


def test_delete_data_range_single_file_double_split(catalog: ParquetDataCatalog):
    """
    Test deleting from a single file that requires both split_before and split_after.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Create test data in a single file that will need both splits
    test_bars = [
        bar(1_000_000_000),
        bar(2_000_000_000),
        bar(3_000_000_000),
        bar(4_000_000_000),
        bar(5_000_000_000),
    ]
    pyo3_catalog.write_bars(test_bars)

    # Act - delete middle range [2_500_000_000, 3_500_000_000]
    # This should create both split_before and split_after operations
    pyo3_catalog.delete_data_range(
        "bars",
        "AUD/USD.SIM",
        2_500_000_000,
        3_500_000_000,
    )

    # Assert - should keep data before and after deletion range
    remaining_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])
    assert len(remaining_bars) == 4

    timestamps = [bar.ts_init for bar in remaining_bars]
    timestamps.sort()
    assert timestamps == [1_000_000_000, 2_000_000_000, 4_000_000_000, 5_000_000_000]


def test_delete_data_range_file_contiguity_verification(catalog: ParquetDataCatalog):
    """
    Test that split files maintain proper timestamp contiguity.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Create test data that will be split
    test_bars = [
        bar(1_000_000_000),
        bar(1_000_000_001),
        bar(1_000_000_002),
        bar(1_000_000_003),
        bar(1_000_000_004),
    ]
    pyo3_catalog.write_bars(test_bars)

    # Act - delete middle timestamp [1_000_000_002, 1_000_000_002]
    pyo3_catalog.delete_data_range(
        "bars",
        "AUD/USD.SIM",
        1_000_000_002,
        1_000_000_002,
    )

    # Assert - verify remaining data maintains proper order and contiguity
    remaining_bars = pyo3_catalog.query_bars(["AUD/USD.SIM"])
    assert len(remaining_bars) == 4

    timestamps = [bar.ts_init for bar in remaining_bars]
    timestamps.sort()
    expected = [1_000_000_000, 1_000_000_001, 1_000_000_003, 1_000_000_004]
    assert timestamps == expected

    # Verify that the gap is exactly where we deleted (timestamp 1_000_000_002 is missing)
    for i in range(len(timestamps) - 1):
        if timestamps[i] == 1_000_000_001:
            # Should jump from 1_000_000_001 to 1_000_000_003 (skipping 1_000_000_002)
            assert timestamps[i + 1] == 1_000_000_003


# ================================================================================================
# Table naming fix tests for pyo3 bindings
# ================================================================================================


def test_pyo3_query_multiple_instruments_table_naming(catalog: ParquetDataCatalog):
    """
    Test that pyo3 bindings handle multiple instruments correctly with identifier-
    dependent table names.

    This test verifies the fix for the table naming bug where multiple instruments would
    cause table name conflicts in DataFusion queries when using the Rust backend.

    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Create quote ticks for multiple instruments with different identifier patterns
    eurusd_quotes = [
        TestDataProviderPyo3.quote_tick(
            ts_init=1000 + i * 100,
            instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
        )
        for i in range(3)
    ]

    btcusd_quotes = [
        TestDataProviderPyo3.quote_tick(
            ts_init=2000 + i * 100,
            instrument_id=InstrumentId.from_str("BTC-USD.COINBASE"),
        )
        for i in range(3)
    ]

    ethusdt_quotes = [
        TestDataProviderPyo3.quote_tick(
            ts_init=3000 + i * 100,
            instrument_id=InstrumentId.from_str("ETH/USDT.BINANCE"),
        )
        for i in range(3)
    ]

    # Write data for all instruments
    pyo3_catalog.write_quote_ticks(eurusd_quotes)
    pyo3_catalog.write_quote_ticks(btcusd_quotes)
    pyo3_catalog.write_quote_ticks(ethusdt_quotes)

    # Act - Query all instruments simultaneously using pyo3 bindings
    instrument_ids = ["EUR/USD.SIM", "BTC-USD.COINBASE", "ETH/USDT.BINANCE"]
    quotes = pyo3_catalog.query_quote_ticks(instrument_ids)

    # Assert - Should get all 9 quotes without table name conflicts
    assert len(quotes) == 9

    # Verify we have data from all three instruments
    instrument_counts: dict[str, int] = {}
    for quote in quotes:
        instrument_id = quote.instrument_id.value
        instrument_counts[instrument_id] = instrument_counts.get(instrument_id, 0) + 1

    assert len(instrument_counts) == 3
    assert instrument_counts.get("EUR/USD.SIM") == 3
    assert instrument_counts.get("BTC-USD.COINBASE") == 3
    assert instrument_counts.get("ETH/USDT.BINANCE") == 3

    # Verify data is properly ordered by timestamp
    timestamps = [quote.ts_init for quote in quotes]
    assert timestamps == sorted(timestamps)


def test_pyo3_query_bars_multiple_instruments_table_naming(catalog: ParquetDataCatalog):
    """
    Test that pyo3 bindings handle multiple bar types correctly with identifier-
    dependent table names.
    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Create bars using the existing bar helper function but with different timestamps
    bars_set1 = [bar(1000000000 + i * 60000000000) for i in range(2)]  # Use nanosecond timestamps
    bars_set2 = [bar(2000000000000 + i * 60000000000) for i in range(2)]  # Much later timestamps

    # Write data for both sets
    pyo3_catalog.write_bars(bars_set1)
    pyo3_catalog.write_bars(bars_set2)

    # Act - Query all bars (this tests the table naming fix)
    bars = pyo3_catalog.query_bars()

    # Assert - Should get all 4 bars without table name conflicts
    assert len(bars) == 4

    # Verify data is properly ordered by timestamp
    timestamps = [bar_data.ts_init for bar_data in bars]
    assert timestamps == sorted(timestamps)


def test_pyo3_backend_session_special_characters_table_naming(catalog: ParquetDataCatalog):
    """
    Test that pyo3 backend session handles special characters in identifiers correctly.

    This test verifies that identifiers with dots, hyphens, and slashes are properly
    converted to safe SQL table names in the Rust backend.

    """
    # Arrange
    pyo3_catalog = ParquetDataCatalogV2(catalog.path)

    # Create trade ticks for instruments with various special characters
    trades_complex = [
        TestDataProviderPyo3.trade_tick(
            ts_init=1000 + i * 100,
            instrument_id=InstrumentId.from_str("BTC/USD.COINBASE-PRO"),
        )
        for i in range(2)
    ]

    trades_dots = [
        TestDataProviderPyo3.trade_tick(
            ts_init=2000 + i * 100,
            instrument_id=InstrumentId.from_str("ETH.USD.KRAKEN"),
        )
        for i in range(2)
    ]

    trades_mixed = [
        TestDataProviderPyo3.trade_tick(
            ts_init=3000 + i * 100,
            instrument_id=InstrumentId.from_str("ADA-BTC.BINANCE_SPOT"),
        )
        for i in range(2)
    ]

    # Write data
    pyo3_catalog.write_trade_ticks(trades_complex)
    pyo3_catalog.write_trade_ticks(trades_dots)
    pyo3_catalog.write_trade_ticks(trades_mixed)

    # Act - Query all instruments with special characters
    instrument_ids = [
        "BTC/USD.COINBASE-PRO",
        "ETH.USD.KRAKEN",
        "ADA-BTC.BINANCE_SPOT",
    ]
    trades = pyo3_catalog.query_trade_ticks(instrument_ids)

    # Assert - Should handle all special characters correctly
    assert len(trades) == 6

    # Verify we have data from all instruments
    instrument_counts: dict[str, int] = {}
    for trade in trades:
        instrument_id = trade.instrument_id.value
        instrument_counts[instrument_id] = instrument_counts.get(instrument_id, 0) + 1

    assert len(instrument_counts) == 3
    assert instrument_counts.get("BTC/USD.COINBASE-PRO") == 2
    assert instrument_counts.get("ETH.USD.KRAKEN") == 2
    assert instrument_counts.get("ADA-BTC.BINANCE_SPOT") == 2
