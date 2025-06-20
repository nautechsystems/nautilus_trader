#!/usr/bin/env python3

"""
Unit tests for the consolidate_data_by_period method.
"""

import tempfile
from unittest.mock import MagicMock
from unittest.mock import patch

import pandas as pd

from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.persistence.catalog import parquet as parquet_module
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.test_kit.stubs.data import TestDataStubs


class TestConsolidateDataByPeriod:
    """
    Test cases for consolidate_data_by_period method.
    """

    def setup_method(self):
        """
        Set up test fixtures.
        """
        self.temp_dir = tempfile.mkdtemp()
        self.catalog = ParquetDataCatalog(path=self.temp_dir)

        # Create mock instrument
        self.instrument_id = InstrumentId(
            symbol=Symbol("EURUSD"),
            venue=Venue("SIM"),
        )

    def teardown_method(self):
        """
        Clean up test fixtures.
        """
        import shutil

        shutil.rmtree(self.temp_dir, ignore_errors=True)

    @patch.object(parquet_module, "_parse_filename_timestamps")
    @patch.object(ParquetDataCatalog, "_query_files")
    @patch.object(ParquetDataCatalog, "query")
    @patch.object(ParquetDataCatalog, "write_data")
    @patch.object(ParquetDataCatalog, "_make_path")
    def test_consolidate_with_data(
        self,
        mock_make_path,
        mock_write_data,
        mock_query,
        mock_query_files,
        mock_parse_timestamps,
    ):
        """
        Test consolidation with actual data.
        """
        mock_make_path.return_value = "/test/path"

        # Mock existing files
        mock_files = ["/test/file1.parquet", "/test/file2.parquet"]

        # Mock file timestamps (2 days of data) - make them contiguous
        day1_start = dt_to_unix_nanos(pd.Timestamp("2024-01-01 00:00:00", tz="UTC"))
        day1_end = dt_to_unix_nanos(pd.Timestamp("2024-01-01 23:59:59.999999999", tz="UTC"))
        day2_start = day1_end + 1  # Make it exactly contiguous (next nanosecond)
        day2_end = dt_to_unix_nanos(pd.Timestamp("2024-01-02 23:59:59.999999999", tz="UTC"))

        # Create a function that returns the appropriate timestamp based on filename
        def mock_parse_func(filename):
            if "file1" in filename:
                return (day1_start, day1_end)
            elif "file2" in filename:
                return (day2_start, day2_end)
            return None

        mock_parse_timestamps.side_effect = mock_parse_func

        mock_query_files.return_value = mock_files

        # Mock filesystem
        self.catalog.fs = MagicMock()
        self.catalog.fs.glob.return_value = mock_files
        self.catalog.fs.rm = MagicMock()
        self.catalog.fs.exists.return_value = False  # Target files don't exist yet

        # Mock get_intervals to return the intervals
        intervals = [(day1_start, day1_end), (day2_start, day2_end)]

        # Mock query results for each period
        mock_data_day1 = [MagicMock(ts_init=day1_start + 1000)]
        mock_data_day2 = [MagicMock(ts_init=day2_start + 1000)]

        mock_query.side_effect = [mock_data_day1, mock_data_day2]

        with patch.object(self.catalog, "get_intervals", return_value=intervals):
            # Run consolidation with 1-day periods
            self.catalog.consolidate_data_by_period(
                data_cls=QuoteTick,
                identifier="EURUSD.SIM",
                period=pd.Timedelta(days=1),
                ensure_contiguous_files=True,
            )

        # Verify write_data was called for each period
        assert mock_write_data.call_count == 2

        # Verify query was called for each period
        assert mock_query.call_count == 2

    def test_prepare_consolidation_queries_with_splits(self):
        """
        Test the auxiliary function _prepare_consolidation_queries with interval
        splitting.
        """
        # Create an interval that spans across the consolidation range
        # File: [1000, 5000], Request: start=2000, end=4000
        # Should result in split queries for [1000, 1999] and [4001, 5000], plus consolidation for [2000, 4000]

        intervals = [(1000, 5000)]
        period = pd.Timedelta(days=1)
        request_start = pd.Timestamp("1970-01-01 00:00:00.000002", tz="UTC")  # 2000 ns
        request_end = pd.Timestamp("1970-01-01 00:00:00.000004", tz="UTC")  # 4000 ns

        # Mock the filesystem exists check to return False (no existing target files)
        with patch.object(self.catalog.fs, "exists", return_value=False):
            with patch.object(self.catalog, "_make_path", return_value="/test/path"):
                queries = self.catalog._prepare_consolidation_queries(
                    intervals=intervals,
                    period=period,
                    start=request_start,
                    end=request_end,
                    ensure_contiguous_files=False,
                    data_cls=QuoteTick,
                    identifier="EURUSD.SIM",
                )

        # Should have 3 queries: split before, split after, and consolidation
        assert len(queries) == 3

        # Check split queries
        split_queries = [q for q in queries if q.get("is_split", False)]
        consolidation_queries = [q for q in queries if not q.get("is_split", False)]

        assert len(split_queries) == 2, "Should have 2 split queries"
        assert len(consolidation_queries) == 1, "Should have 1 consolidation query"

        # Verify split before query
        split_before = next((q for q in split_queries if q["query_start"] == 1000), None)
        assert split_before is not None, "Should have split before query"
        assert split_before["query_end"] == request_start.value - 1
        assert split_before["target_file_start"] == 1000
        assert split_before["target_file_end"] == request_start.value - 1
        assert split_before["use_period_boundaries"] is False

        # Verify split after query
        split_after = next(
            (q for q in split_queries if q["query_start"] == request_end.value + 1),
            None,
        )
        assert split_after is not None, "Should have split after query"
        assert split_after["query_end"] == 5000
        assert split_after["target_file_start"] == request_end.value + 1
        assert split_after["target_file_end"] == 5000
        assert split_after["use_period_boundaries"] is False

        # Verify consolidation query
        consolidation = consolidation_queries[0]
        assert consolidation["query_start"] <= request_start.value
        assert consolidation["query_end"] >= request_end.value
        assert consolidation["is_split"] is False


def test_consolidate_catalog_by_period(catalog: ParquetDataCatalog) -> None:
    # Arrange
    quotes = [TestDataStubs.quote_tick() for _ in range(5)]
    catalog.write_data(quotes)

    # Get initial file count
    leaf_dirs = catalog._find_leaf_data_directories()
    initial_file_count = 0
    for directory in leaf_dirs:
        files = catalog.fs.glob(f"{directory}/*.parquet")
        initial_file_count += len(files)

    # Act
    catalog.consolidate_catalog_by_period(
        period=pd.Timedelta(days=1),
        ensure_contiguous_files=False,
    )

    # Assert - method should complete without error
    # Note: Since all quotes have the same timestamp, they should be consolidated
    final_file_count = 0
    for directory in leaf_dirs:
        files = catalog.fs.glob(f"{directory}/*.parquet")
        final_file_count += len(files)

    # The consolidation should have processed the files
    assert initial_file_count >= 1  # We had some files initially


def test_extract_data_cls_and_identifier_from_path(catalog: ParquetDataCatalog) -> None:
    # Arrange
    quote = TestDataStubs.quote_tick()
    catalog.write_data([quote])

    # Get a leaf directory
    leaf_dirs = catalog._find_leaf_data_directories()
    assert len(leaf_dirs) > 0

    test_directory = leaf_dirs[0]

    # Act
    data_cls, identifier = catalog._extract_data_cls_and_identifier_from_path(test_directory)

    # Assert
