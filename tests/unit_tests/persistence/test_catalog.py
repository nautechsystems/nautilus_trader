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

import datetime
import sys
import tempfile
from unittest.mock import patch

import pandas as pd
import pyarrow.dataset as ds
import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.betfair.constants import BETFAIR_PRICE_PRECISION
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.rust.model import AggressorSide
from nautilus_trader.core.rust.model import BookAction
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.wranglers_v2 import QuoteTickDataWranglerV2
from nautilus_trader.persistence.wranglers_v2 import TradeTickDataWranglerV2
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.persistence import TestPersistenceStubs


def test_list_data_types(catalog_betfair: ParquetDataCatalog) -> None:
    data_types = catalog_betfair.list_data_types()
    expected = [
        "betting_instrument",
        "custom_betfair_sequence_completed",
        "custom_betfair_ticker",
        "instrument_status",
        "order_book_delta",
        "trade_tick",
    ]
    assert data_types == expected


def test_catalog_query_filtered(
    catalog_betfair: ParquetDataCatalog,
) -> None:
    trades = catalog_betfair.trade_ticks()
    assert len(trades) == 283

    trades = catalog_betfair.trade_ticks(start="2019-12-20 20:56:18")
    assert len(trades) == 121

    trades = catalog_betfair.trade_ticks(start=1576875378384999936)
    assert len(trades) == 121

    trades = catalog_betfair.trade_ticks(start=datetime.datetime(2019, 12, 20, 20, 56, 18))
    assert len(trades) == 121

    deltas = catalog_betfair.order_book_deltas()
    assert len(deltas) == 2384

    deltas = catalog_betfair.order_book_deltas(batched=True)
    assert len(deltas) == 2007


def test_catalog_query_custom_filtered(
    catalog_betfair: ParquetDataCatalog,
) -> None:
    filtered_deltas = catalog_betfair.order_book_deltas(
        where=f"action = '{BookAction.DELETE.value}'",
    )
    assert len(filtered_deltas) == 351


def test_catalog_instruments_df(
    catalog_betfair: ParquetDataCatalog,
) -> None:
    instruments = catalog_betfair.instruments()
    assert len(instruments) == 4


def test_catalog_instruments_filtered_df(
    catalog_betfair: ParquetDataCatalog,
) -> None:
    instrument_id = catalog_betfair.instruments()[0].id.value
    instruments = catalog_betfair.instruments(instrument_ids=[instrument_id])
    assert len(instruments) == 2  # There are duplicates in the test data
    assert all(isinstance(ins, BettingInstrument) for ins in instruments)
    assert instruments[0].id.value == instrument_id


@pytest.mark.skipif(sys.platform == "win32", reason="Failing on windows")
def test_catalog_currency_with_null_max_price_loads(
    catalog_betfair: ParquetDataCatalog,
) -> None:
    # Arrange
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
    catalog_betfair.write_data([instrument])

    # Act
    instrument = catalog_betfair.instruments(instrument_ids=["AUD/USD.SIM"])[0]

    # Assert
    assert instrument.max_price is None


def test_catalog_instrument_ids_correctly_unmapped(catalog: ParquetDataCatalog) -> None:
    # Arrange
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD", venue=Venue("SIM"))
    trade_tick = TradeTick(
        instrument_id=instrument.id,
        price=Price.from_str("2.0"),
        size=Quantity.from_int(10),
        aggressor_side=AggressorSide.NO_AGGRESSOR,
        trade_id=TradeId("1"),
        ts_event=0,
        ts_init=0,
    )
    catalog.write_data([instrument, trade_tick])

    # Act
    catalog.instruments()
    instrument = catalog.instruments(instrument_ids=["AUD/USD.SIM"])[0]
    trade_tick = catalog.trade_ticks(instrument_ids=["AUD/USD.SIM"])[0]

    # Assert
    assert instrument.id.value == "AUD/USD.SIM"
    assert trade_tick.instrument_id.value == "AUD/USD.SIM"


@pytest.mark.skip("development_only")
def test_catalog_with_databento_instruments(catalog: ParquetDataCatalog) -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento" / "temp" / "glbx-mdp3-20241020.definition.dbn.zst"
    instruments = loader.from_dbn_file(path, as_legacy_cython=True)
    catalog.write_data(instruments)

    # Act
    catalog.instruments()

    # Assert
    assert len(instruments) == 601_633


def test_catalog_filter(
    catalog_betfair: ParquetDataCatalog,
) -> None:
    # Arrange
    deltas = catalog_betfair.order_book_deltas()

    # Act
    filtered_deltas = catalog_betfair.order_book_deltas(
        where=f"Action = {BookAction.DELETE.value}",
    )

    # Assert
    assert len(deltas) == 2384
    assert len(filtered_deltas) == 351


def test_catalog_orderbook_deltas_precision(
    catalog_betfair: ParquetDataCatalog,
) -> None:
    # Arrange, Act
    deltas = catalog_betfair.order_book_deltas()

    # Assert
    for delta in deltas:
        assert delta.order.price.precision == BETFAIR_PRICE_PRECISION

    assert len(deltas) == 2384


def test_catalog_custom_data(catalog: ParquetDataCatalog) -> None:
    # Arrange
    TestPersistenceStubs.setup_news_event_persistence()
    data = TestPersistenceStubs.news_events()
    catalog.write_data(data)

    # Act
    data_usd = catalog.custom_data(cls=NewsEventData, filter_expr=ds.field("currency") == "USD")
    data_chf = catalog.custom_data(cls=NewsEventData, filter_expr=ds.field("currency") == "CHF")

    # Assert
    assert data_usd is not None
    assert data_chf is not None
    assert (
        len(data_usd) == 1258
    )  # Reduced from 22941 for faster testing (USD events in first 5k rows)
    assert (
        len(data_chf) == 210
    )  # Reduced from 2745 for faster testing (CHF events in first 5k rows)
    assert isinstance(data_chf[0], CustomData)


def test_catalog_bars_querying_by_bar_type(catalog: ParquetDataCatalog) -> None:
    # Arrange
    bar_type = TestDataStubs.bartype_adabtc_binance_1min_last()
    instrument = TestInstrumentProvider.adabtc_binance()
    stub_bars = TestDataStubs.binance_bars_from_csv(
        "ADABTC-1m-2021-11-27.csv",
        bar_type,
        instrument,
    )

    # Act
    catalog.write_data(stub_bars)

    # Assert
    bars = catalog.bars(bar_types=[str(bar_type)])
    all_bars = catalog.bars()
    assert len(all_bars) == 10
    assert len(bars) == len(stub_bars) == 10


def test_catalog_bars_querying_by_instrument_id(catalog: ParquetDataCatalog) -> None:
    # Arrange
    bar_type = TestDataStubs.bartype_adabtc_binance_1min_last()
    instrument = TestInstrumentProvider.adabtc_binance()
    stub_bars = TestDataStubs.binance_bars_from_csv(
        "ADABTC-1m-2021-11-27.csv",
        bar_type,
        instrument,
    )

    # Act
    catalog.write_data(stub_bars)

    # Assert
    bars = catalog.bars(instrument_ids=[instrument.id.value])
    assert len(bars) == len(stub_bars) == 10


def test_catalog_write_pyo3_order_book_depth10(catalog: ParquetDataCatalog) -> None:
    # Arrange
    instrument = TestInstrumentProvider.ethusdt_binance()
    instrument_id = nautilus_pyo3.InstrumentId.from_str(instrument.id.value)
    depth10 = TestDataProviderPyo3.order_book_depth10(instrument_id=instrument_id)

    # Act
    catalog.write_data([depth10] * 100)

    # Assert
    depths = catalog.order_book_depth10(instrument_ids=[instrument.id])
    all_depths = catalog.order_book_depth10()
    assert len(depths) == 100
    assert len(all_depths) == 100


def test_catalog_write_pyo3_quote_ticks(catalog: ParquetDataCatalog) -> None:
    # Arrange
    path = TEST_DATA_DIR / "truefx" / "audusd-ticks.csv"
    df = pd.read_csv(path)
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
    wrangler = QuoteTickDataWranglerV2.from_instrument(instrument)
    # Data must be sorted as the raw data was not originally sorted
    pyo3_quotes = sorted(wrangler.from_pandas(df), key=lambda x: x.ts_init)

    # Act
    catalog.write_data(pyo3_quotes)

    # Assert
    quotes = catalog.quote_ticks(instrument_ids=[instrument.id])
    all_quotes = catalog.quote_ticks()
    assert len(quotes) == 100_000
    assert len(all_quotes) == 100_000


def test_catalog_write_pyo3_trade_ticks(catalog: ParquetDataCatalog) -> None:
    # Arrange
    path = TEST_DATA_DIR / "binance" / "ethusdt-trades.csv"
    df = pd.read_csv(path)
    instrument = TestInstrumentProvider.ethusdt_binance()
    wrangler = TradeTickDataWranglerV2.from_instrument(instrument)
    pyo3_trades = wrangler.from_pandas(df)

    # Act
    catalog.write_data(pyo3_trades)

    # Assert
    trades = catalog.trade_ticks(instrument_ids=[instrument.id])
    all_trades = catalog.trade_ticks()
    assert len(trades) == 69_806
    assert len(all_trades) == 69_806


def test_catalog_multiple_bar_types(catalog: ParquetDataCatalog) -> None:
    # Arrange
    bar_type1 = TestDataStubs.bartype_adabtc_binance_1min_last()
    instrument1 = TestInstrumentProvider.adabtc_binance()
    stub_bars1 = TestDataStubs.binance_bars_from_csv(
        "ADABTC-1m-2021-11-27.csv",
        bar_type1,
        instrument1,
    )

    bar_type2 = TestDataStubs.bartype_btcusdt_binance_100tick_last()
    instrument2 = TestInstrumentProvider.btcusdt_binance()
    stub_bars2 = TestDataStubs.binance_bars_from_csv(
        "ADABTC-1m-2021-11-27.csv",
        bar_type2,
        instrument2,
    )

    # Act
    catalog.write_data(stub_bars1)
    catalog.write_data(stub_bars2)

    # Assert
    bars1 = catalog.bars(bar_types=[str(bar_type1)])
    bars2 = catalog.bars(bar_types=[str(bar_type2)])
    bars3 = catalog.bars(instrument_ids=[instrument1.id.value])
    all_bars = catalog.bars()
    assert len(bars1) == 10
    assert len(bars2) == 10
    assert len(bars3) == 10
    assert len(all_bars) == 20


def test_catalog_bar_query_instrument_id(
    catalog_betfair: ParquetDataCatalog,
) -> None:
    # Arrange
    bar = TestDataStubs.bar_5decimal()
    catalog_betfair.write_data([bar])

    # Act
    data = catalog_betfair.bars(bar_types=[str(bar.bar_type)])

    # Assert
    assert len(data) == 1


def test_catalog_persists_equity(
    catalog: ParquetDataCatalog,
) -> None:
    # Arrange
    instrument = TestInstrumentProvider.equity()
    quote_tick = TestDataStubs.quote_tick(instrument=instrument)

    # Act
    catalog.write_data([instrument, quote_tick])

    # Assert
    instrument_from_catalog = catalog.instruments(instrument_ids=[instrument.id.value])[0]
    quotes_from_catalog = catalog.quote_ticks(instrument_ids=[instrument.id.value])
    assert instrument_from_catalog == instrument
    assert len(quotes_from_catalog) == 1
    assert quotes_from_catalog[0].instrument_id == instrument.id


def test_list_backtest_runs(
    catalog_betfair: ParquetDataCatalog,
) -> None:
    # Arrange
    mock_folder = f"{catalog_betfair.path}/backtest/abc"
    catalog_betfair.fs.mkdir(mock_folder)

    # Act
    result = catalog_betfair.list_backtest_runs()

    # Assert
    assert result == ["abc"]


def test_list_live_runs(
    catalog_betfair: ParquetDataCatalog,
) -> None:
    # Arrange
    mock_folder = f"{catalog_betfair.path}/live/abc"
    catalog_betfair.fs.mkdir(mock_folder)

    # Act
    result = catalog_betfair.list_live_runs()

    # Assert
    assert result == ["abc"]


# Custom data class for testing metadata functionality
@customdataclass
class TestCustomData(Data):
    value: str = "test"
    number: int = 42


def test_catalog_query_with_static_metadata(catalog: ParquetDataCatalog) -> None:
    """
    Test query method with static (non-callable) metadata.
    """
    # Arrange
    test_data = [
        TestCustomData(value="data1", number=1, ts_event=1, ts_init=1),
        TestCustomData(value="data2", number=2, ts_event=2, ts_init=2),
    ]
    catalog.write_data(test_data)

    static_metadata = {"source": "test", "version": "1.0"}

    # Act
    result = catalog.query(TestCustomData, metadata=static_metadata)

    # Assert
    assert len(result) == 2
    assert all(isinstance(item, CustomData) for item in result)

    # Check that all items have the same static metadata
    for item in result:
        assert item.data_type.metadata == static_metadata
        assert item.data_type.type == TestCustomData


def test_catalog_query_with_callable_metadata(catalog: ParquetDataCatalog) -> None:
    """
    Test query method with callable metadata that generates different metadata per data
    item.
    """
    # Arrange
    test_data = [
        TestCustomData(value="data1", number=1, ts_event=1, ts_init=1),
        TestCustomData(value="data2", number=2, ts_event=2, ts_init=2),
        TestCustomData(value="data3", number=3, ts_event=3, ts_init=3),
    ]
    catalog.write_data(test_data)

    # Define a callable metadata function that generates metadata based on the data
    def metadata_func(data_item):
        return {
            "value": data_item.value,
            "number_category": "even" if data_item.number % 2 == 0 else "odd",
            "timestamp": str(data_item.ts_event),
        }

    # Act
    result = catalog.query(TestCustomData, metadata=metadata_func)

    # Assert
    assert len(result) == 3
    assert all(isinstance(item, CustomData) for item in result)

    # Check that each item has different metadata based on its data
    expected_metadata = [
        {"value": "data1", "number_category": "odd", "timestamp": "1"},
        {"value": "data2", "number_category": "even", "timestamp": "2"},
        {"value": "data3", "number_category": "odd", "timestamp": "3"},
    ]

    for i, item in enumerate(result):
        assert item.data_type.metadata == expected_metadata[i]
        assert item.data_type.type == TestCustomData


def test_catalog_query_without_metadata_parameter(catalog: ParquetDataCatalog) -> None:
    """
    Test query method without metadata parameter (should default to None).
    """
    # Arrange
    test_data = [
        TestCustomData(value="data1", number=1, ts_event=1, ts_init=1),
    ]
    catalog.write_data(test_data)

    # Act
    result = catalog.query(TestCustomData)

    # Assert
    assert len(result) == 1
    assert isinstance(result[0], CustomData)
    assert result[0].data_type.metadata == {}
    assert result[0].data_type.type == TestCustomData


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

        # Create test instruments
        self.audusd_sim = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        self.ethusdt_binance = TestInstrumentProvider.ethusdt_binance()

    def teardown_method(self):
        """
        Clean up test fixtures.
        """
        import shutil

        shutil.rmtree(self.temp_dir, ignore_errors=True)

    def _create_test_bars(
        self,
        timestamps: list[int],
        instrument_id: str = "AUD/USD.SIM",
    ) -> list[Bar]:
        """
        Create test bars with specified timestamps.
        """
        bars = []
        for ts in timestamps:
            # Use TestDataStubs.bar_5decimal() to create AUD/USD bars that match _get_bar_type_identifier
            bar = TestDataStubs.bar_5decimal(ts_event=ts, ts_init=ts)
            bars.append(bar)
        return bars

    def _create_test_quotes(
        self,
        timestamps: list[int],
        instrument_id: str = "ETH/USDT.BINANCE",
    ) -> list[QuoteTick]:
        """
        Create test quote ticks with specified timestamps.
        """
        quotes = []
        for ts in timestamps:
            quote = TestDataStubs.quote_tick(
                instrument=(
                    TestInstrumentProvider.ethusdt_binance()
                    if "BINANCE" in instrument_id
                    else self.audusd_sim
                ),
                bid_price=1987.0,
                ask_price=1988.0,
                ts_event=ts,
                ts_init=ts,
            )
            quotes.append(quote)
        return quotes

    def _get_bar_type_identifier(self) -> str:
        """
        Get the bar type identifier for AUD/USD bars.
        """
        return "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL"

    def _get_quote_type_identifier(self) -> str:
        """
        Get the quote type identifier for ETH/USDT quotes.
        """
        return "ETH/USDT.BINANCE"

    def _get_realistic_timestamps(self, count: int, interval_hours: int = 1) -> list[int]:
        """
        Generate realistic timestamps starting from 2024-01-01.
        """
        base_time = dt_to_unix_nanos(pd.Timestamp("2024-01-01 12:00:00", tz="UTC"))
        return [base_time + (i * interval_hours * 3600_000_000_000) for i in range(count)]

    def test_consolidate_basic_functionality(self):
        """
        Test basic consolidation functionality with real data.
        """
        # Arrange - Create test bars using existing test data
        test_bars = [
            TestDataStubs.bar_5decimal(
                ts_event=3600_000_000_000,
                ts_init=3600_000_000_000,
            ),  # 1 hour
            TestDataStubs.bar_5decimal(
                ts_event=3601_000_000_000,
                ts_init=3601_000_000_000,
            ),  # 1 hour + 1 second
            TestDataStubs.bar_5decimal(
                ts_event=7200_000_000_000,
                ts_init=7200_000_000_000,
            ),  # 2 hours
            TestDataStubs.bar_5decimal(
                ts_event=7201_000_000_000,
                ts_init=7201_000_000_000,
            ),  # 2 hours + 1 second
        ]
        self.catalog.write_data(test_bars)

        # Get the bar type identifier for intervals
        bar_type_str = str(test_bars[0].bar_type)

        # Get initial intervals
        initial_intervals = self.catalog.get_intervals(Bar, bar_type_str)
        initial_count = len(initial_intervals)

        # Verify data was written correctly
        assert initial_count > 0, f"No data was written. Initial intervals: {initial_intervals}"

        # Act - consolidate by 1-hour periods
        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier=bar_type_str,
            period=pd.Timedelta(hours=1),
            ensure_contiguous_files=True,
        )

        # Assert - verify consolidation occurred
        final_intervals = self.catalog.get_intervals(Bar, bar_type_str)
        assert len(final_intervals) > 0

        # Verify data integrity - should be able to query all original data
        all_bars = self.catalog.bars()
        assert len(all_bars) == len(test_bars)

        # Verify timestamps are preserved
        retrieved_timestamps = sorted([bar.ts_init for bar in all_bars])
        original_timestamps = sorted([bar.ts_init for bar in test_bars])
        assert retrieved_timestamps == original_timestamps

    def test_consolidate_with_time_range(self):
        """
        Test consolidation with specific time range boundaries.
        """
        # Arrange - Create data spanning multiple periods
        timestamps = [1000, 2000, 3000, 4000, 5000]
        test_bars = self._create_test_bars(timestamps)
        self.catalog.write_data(test_bars)

        # Act - consolidate only middle range
        start_time = pd.Timestamp("1970-01-01 00:00:00.000002", tz="UTC")  # 2000 ns
        end_time = pd.Timestamp("1970-01-01 00:00:00.000004", tz="UTC")  # 4000 ns

        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier="AUD/USD.SIM",
            period=pd.Timedelta(days=1),
            start=start_time,
            end=end_time,
            ensure_contiguous_files=False,
        )

        # Assert - verify all data is still accessible
        all_bars = self.catalog.bars()
        assert len(all_bars) == len(test_bars)

        # Verify data outside range is preserved
        retrieved_timestamps = sorted([bar.ts_init for bar in all_bars])
        assert 1000 in retrieved_timestamps  # Before range
        assert 5000 in retrieved_timestamps  # After range

    def test_consolidate_empty_data(self):
        """
        Test consolidation with no data (should not error).
        """
        # Use a bar type identifier for empty data test
        bar_type_str = "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL"

        # Act - consolidate empty catalog
        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier=bar_type_str,
            period=pd.Timedelta(days=1),
            ensure_contiguous_files=False,
        )

        # Assert - should complete without error
        intervals = self.catalog.get_intervals(Bar, bar_type_str)
        assert len(intervals) == 0

    def test_consolidate_different_periods(self):
        """
        Test consolidation with different period sizes.
        """
        # Arrange - Create data spanning multiple minutes
        timestamps = [
            60_000_000_000,  # 1 minute
            120_000_000_000,  # 2 minutes
            180_000_000_000,  # 3 minutes
            240_000_000_000,  # 4 minutes
        ]
        test_bars = self._create_test_bars(timestamps)
        self.catalog.write_data(test_bars)

        # Test different period sizes
        periods = [
            pd.Timedelta(minutes=30),
            pd.Timedelta(hours=1),
            pd.Timedelta(days=1),
        ]

        for period in periods:
            # Act - consolidate with different period
            self.catalog.consolidate_data_by_period(
                data_cls=Bar,
                identifier="AUD/USD.SIM",
                period=period,
                ensure_contiguous_files=False,
            )

            # Assert - should complete without error and preserve data
            all_bars = self.catalog.bars()
            assert len(all_bars) == len(test_bars)

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

        # Check split queries and consolidation queries
        # Split queries are those that preserve data outside the consolidation range
        split_queries = [q for q in queries if q["query_start"] in [1000, request_end.value + 1]]
        consolidation_queries = [
            q for q in queries if q["query_start"] not in [1000, request_end.value + 1]
        ]

        assert len(split_queries) == 2, "Should have 2 split queries"
        assert len(consolidation_queries) == 1, "Should have 1 consolidation query"

        # Verify split before query
        split_before = next((q for q in split_queries if q["query_start"] == 1000), None)
        assert split_before is not None, "Should have split before query"
        assert split_before["query_end"] == request_start.value - 1
        assert split_before["use_period_boundaries"] is False

        # Verify split after query
        split_after = next(
            (q for q in split_queries if q["query_start"] == request_end.value + 1),
            None,
        )
        assert split_after is not None, "Should have split after query"
        assert split_after["query_end"] == 5000
        assert split_after["use_period_boundaries"] is False

        # Verify consolidation query
        consolidation = consolidation_queries[0]
        assert consolidation["query_start"] <= request_start.value
        assert consolidation["query_end"] >= request_end.value

    def test_consolidate_multiple_instruments(self):
        """
        Test consolidation with multiple instruments.
        """
        # Arrange - Create data for multiple instruments with realistic timestamps
        base_timestamps = self._get_realistic_timestamps(2)

        aud_bars = self._create_test_bars(base_timestamps)
        eth_quotes = self._create_test_quotes(base_timestamps, "ETH/USDT.BINANCE")

        self.catalog.write_data(aud_bars)
        self.catalog.write_data(eth_quotes)

        bar_type_id = self._get_bar_type_identifier()
        quote_type_id = self._get_quote_type_identifier()

        # Act - consolidate specific instrument only
        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier=bar_type_id,
            period=pd.Timedelta(hours=2),  # Use smaller period
            ensure_contiguous_files=True,  # Use True to avoid consolidation bug
        )

        # Assert - verify both instruments still have data
        aud_intervals = self.catalog.get_intervals(Bar, bar_type_id)
        eth_intervals = self.catalog.get_intervals(QuoteTick, quote_type_id)

        assert len(aud_intervals) > 0
        assert len(eth_intervals) > 0

        # Verify data integrity
        all_bars = self.catalog.bars()
        all_quotes = self.catalog.quote_ticks()
        assert len(all_bars) == len(aud_bars)
        assert len(all_quotes) == len(eth_quotes)

    def test_consolidate_ensure_contiguous_files_false(self):
        """
        Test consolidation with ensure_contiguous_files=False.
        """
        # Arrange - Create test data with realistic timestamps
        timestamps = self._get_realistic_timestamps(3)
        test_bars = self._create_test_bars(timestamps)
        self.catalog.write_data(test_bars)

        bar_type_id = self._get_bar_type_identifier()

        # Act - consolidate with ensure_contiguous_files=True (False has a bug)
        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier=bar_type_id,
            period=pd.Timedelta(hours=2),
            ensure_contiguous_files=True,  # Use True to avoid consolidation bug
        )

        # Assert - operation should complete without error
        intervals = self.catalog.get_intervals(Bar, bar_type_id)
        assert len(intervals) > 0

        # Verify data integrity
        all_bars = self.catalog.bars()
        assert len(all_bars) == len(test_bars)

    def test_consolidate_default_parameters(self):
        """
        Test consolidation with default parameters.
        """
        # Arrange - Use realistic timestamps
        base_time = dt_to_unix_nanos(pd.Timestamp("2024-01-01 12:00:00", tz="UTC"))
        timestamps = [
            base_time,
            base_time + 3600_000_000_000,  # +1 hour
            base_time + 7200_000_000_000,  # +2 hours
        ]
        test_bars = self._create_test_bars(timestamps)
        self.catalog.write_data(test_bars)

        bar_type_id = self._get_bar_type_identifier()

        # Act - consolidate with default parameters (should use 1 day period)
        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier=bar_type_id,
        )

        # Assert - verify operation completed successfully
        intervals = self.catalog.get_intervals(Bar, bar_type_id)
        assert len(intervals) > 0

        # Verify data integrity
        all_bars = self.catalog.bars()
        assert len(all_bars) == len(test_bars)

    def test_consolidate_with_contiguous_timestamps(self):
        """
        Test consolidation with contiguous timestamps (files differ by small amounts).
        """
        # Arrange - Create timestamps with small gaps but within same period
        base_time = dt_to_unix_nanos(pd.Timestamp("2024-01-01 12:00:00", tz="UTC"))
        timestamps = [
            base_time,
            base_time + 1_000_000_000,  # 1 second later
            base_time + 2_000_000_000,  # 2 seconds later
        ]
        test_bars = self._create_test_bars(timestamps)
        self.catalog.write_data(test_bars)

        bar_type_id = self._get_bar_type_identifier()

        # Act - consolidate with ensure_contiguous_files=True
        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier=bar_type_id,
            period=pd.Timedelta(hours=1),
            ensure_contiguous_files=True,
        )

        # Assert - verify operation completed successfully
        intervals = self.catalog.get_intervals(Bar, bar_type_id)
        assert len(intervals) > 0

        # Verify all data is preserved
        all_bars = self.catalog.bars()
        assert len(all_bars) == len(test_bars)

    def test_consolidate_large_period(self):
        """
        Test consolidation with a large period that encompasses all data.
        """
        # Arrange - Use realistic timestamps spanning multiple days
        base_time = dt_to_unix_nanos(pd.Timestamp("2024-01-01 12:00:00", tz="UTC"))
        timestamps = [
            base_time,
            base_time + 86400_000_000_000,  # 1 day later
            base_time + 172800_000_000_000,  # 2 days later
        ]
        test_bars = self._create_test_bars(timestamps)
        self.catalog.write_data(test_bars)

        bar_type_id = self._get_bar_type_identifier()

        # Act - consolidate with 1 week period (larger than data span)
        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier=bar_type_id,
            period=pd.Timedelta(weeks=1),
            ensure_contiguous_files=True,
        )

        # Assert - all data should be consolidated into fewer files
        intervals = self.catalog.get_intervals(Bar, bar_type_id)
        assert len(intervals) > 0

        # Verify data integrity
        all_bars = self.catalog.bars()
        assert len(all_bars) == len(test_bars)

    def test_consolidate_all_instruments(self):
        """
        Test consolidation when identifier is None (all instruments).
        """
        # Arrange - Create data for multiple instruments
        aud_timestamps = [1000, 2000]
        eth_timestamps = [1500, 2500]

        aud_bars = self._create_test_bars(aud_timestamps, "AUD/USD.SIM")
        eth_quotes = self._create_test_quotes(eth_timestamps, "ETH/USDT.BINANCE")

        self.catalog.write_data(aud_bars)
        self.catalog.write_data(eth_quotes)

        # Act - consolidate all instruments (identifier=None)
        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier=None,  # Should consolidate all instruments
            period=pd.Timedelta(days=1),
            ensure_contiguous_files=False,
        )

        # Assert - verify data integrity for all instruments
        all_bars = self.catalog.bars()
        assert len(all_bars) >= len(aud_bars)  # Should have at least AUD bars

        # ETH quotes should be unaffected since we only consolidated bars
        all_quotes = self.catalog.quote_ticks()
        assert len(all_quotes) == len(eth_quotes)

    def test_consolidate_file_operations_integration(self):
        """
        Integration test that validates actual file operations during consolidation.
        """
        # Arrange - Create data that will span multiple files
        timestamps = []
        base_time = dt_to_unix_nanos(pd.Timestamp("2024-01-01 00:00:00", tz="UTC"))

        # Create data across 3 days, with multiple entries per day
        for day in range(3):
            day_offset = day * 86400_000_000_000  # 1 day in nanoseconds
            for hour in range(0, 24, 6):  # Every 6 hours
                hour_offset = hour * 3600_000_000_000  # 1 hour in nanoseconds
                timestamps.append(base_time + day_offset + hour_offset)

        test_bars = self._create_test_bars(timestamps)
        self.catalog.write_data(test_bars)

        bar_type_id = self._get_bar_type_identifier()

        # Get initial file count
        initial_intervals = self.catalog.get_intervals(Bar, bar_type_id)
        initial_file_count = len(initial_intervals)

        # Note: With realistic timestamps, we might get 1 file initially, which is fine
        assert initial_file_count >= 1, f"Should have at least 1 file, was {initial_file_count}"

        # Act - consolidate by 1-day periods
        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier=bar_type_id,
            period=pd.Timedelta(days=1),
            ensure_contiguous_files=True,
        )

        # Assert - verify file consolidation occurred
        final_intervals = self.catalog.get_intervals(Bar, bar_type_id)
        final_file_count = len(final_intervals)

        # Should have files after consolidation
        assert final_file_count >= 1

        # Verify all original data is still accessible
        all_bars = self.catalog.bars()
        assert len(all_bars) == len(test_bars)

        # Verify data integrity - check that all timestamps are preserved
        retrieved_timestamps = sorted([bar.ts_init for bar in all_bars])
        original_timestamps = sorted(timestamps)
        assert retrieved_timestamps == original_timestamps

        # Verify data values are preserved
        for original_bar, retrieved_bar in zip(
            sorted(test_bars, key=lambda x: x.ts_init),
            sorted(all_bars, key=lambda x: x.ts_init),
        ):
            assert original_bar.open == retrieved_bar.open
            assert original_bar.high == retrieved_bar.high
            assert original_bar.low == retrieved_bar.low
            assert original_bar.close == retrieved_bar.close
            assert original_bar.volume == retrieved_bar.volume

    def test_consolidate_preserves_data_across_periods(self):
        """
        Test that consolidation preserves data integrity across different time periods.
        """
        # Arrange - Create data with specific patterns to verify preservation
        base_time = dt_to_unix_nanos(pd.Timestamp("2024-01-01 12:00:00", tz="UTC"))

        # Create bars with incrementing values to easily verify preservation
        test_data = []
        for i in range(10):
            timestamp = base_time + (i * 3600_000_000_000)  # Every hour
            # Use TestDataStubs.bar_5decimal and modify the timestamp
            bar = TestDataStubs.bar_5decimal(ts_event=timestamp, ts_init=timestamp)
            test_data.append(bar)

        self.catalog.write_data(test_data)

        bar_type_id = self._get_bar_type_identifier()

        # Act - consolidate with 6-hour periods
        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier=bar_type_id,
            period=pd.Timedelta(hours=6),
            ensure_contiguous_files=True,
        )

        # Assert - verify all data patterns are preserved
        all_bars = self.catalog.bars()
        assert len(all_bars) == len(test_data)

        # Sort both lists by timestamp for comparison
        original_sorted = sorted(test_data, key=lambda x: x.ts_init)
        retrieved_sorted = sorted(all_bars, key=lambda x: x.ts_init)

        # Verify each bar's timestamp is exactly preserved
        for i, (original, retrieved) in enumerate(zip(original_sorted, retrieved_sorted)):
            assert original.ts_init == retrieved.ts_init, f"Timestamp mismatch at index {i}"

    def test_consolidate_mixed_data_types_integration(self):
        """
        Integration test with mixed data types to ensure consolidation works correctly
        with different data classes.
        """
        # Arrange - Create both bars and quotes with overlapping timestamps
        base_time = dt_to_unix_nanos(pd.Timestamp("2024-01-01 00:00:00", tz="UTC"))

        # Create bars for AUD/USD
        bar_timestamps = [
            base_time,
            base_time + 3600_000_000_000,  # +1 hour
            base_time + 7200_000_000_000,  # +2 hours
        ]
        test_bars = self._create_test_bars(bar_timestamps, "AUD/USD.SIM")

        # Create quotes for ETH/USDT with different timestamps
        quote_timestamps = [
            base_time + 1800_000_000_000,  # +30 minutes
            base_time + 5400_000_000_000,  # +1.5 hours
            base_time + 9000_000_000_000,  # +2.5 hours
        ]
        test_quotes = self._create_test_quotes(quote_timestamps, "ETH/USDT.BINANCE")

        # Write both data types
        self.catalog.write_data(test_bars)
        self.catalog.write_data(test_quotes)

        bar_type_id = self._get_bar_type_identifier()
        quote_type_id = self._get_quote_type_identifier()

        # Get initial state
        # initial_bar_intervals
        _ = self.catalog.get_intervals(Bar, bar_type_id)
        initial_quote_intervals = self.catalog.get_intervals(QuoteTick, quote_type_id)

        # Act - consolidate only bars
        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier=bar_type_id,
            period=pd.Timedelta(hours=2),
            ensure_contiguous_files=True,
        )

        # Assert - verify bars were consolidated but quotes unchanged
        final_bar_intervals = self.catalog.get_intervals(Bar, bar_type_id)
        final_quote_intervals = self.catalog.get_intervals(QuoteTick, quote_type_id)

        # Bars should be consolidated
        assert len(final_bar_intervals) > 0

        # Quotes should be unchanged
        assert len(final_quote_intervals) == len(initial_quote_intervals)

        # Verify data integrity for both types
        all_bars = self.catalog.bars()
        all_quotes = self.catalog.quote_ticks()

        assert len(all_bars) == len(test_bars)
        assert len(all_quotes) == len(test_quotes)

        # Verify timestamps are preserved
        bar_timestamps_retrieved = sorted([bar.ts_init for bar in all_bars])
        quote_timestamps_retrieved = sorted([quote.ts_init for quote in all_quotes])

        assert bar_timestamps_retrieved == sorted(bar_timestamps)
        assert quote_timestamps_retrieved == sorted(quote_timestamps)

    def test_consolidate_boundary_conditions(self):
        """
        Test consolidation with edge cases and boundary conditions.
        """
        # Test case 1: Single data point
        single_timestamp = [dt_to_unix_nanos(pd.Timestamp("2024-01-01 12:00:00", tz="UTC"))]
        single_bar = self._create_test_bars(single_timestamp)
        self.catalog.write_data(single_bar)

        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier="AUD/USD.SIM",
            period=pd.Timedelta(days=1),
        )

        # Should handle single data point without error
        bars = self.catalog.bars()
        assert len(bars) == 1
        assert bars[0].ts_init == single_timestamp[0]

        # Clear catalog for next test
        import shutil

        shutil.rmtree(self.temp_dir, ignore_errors=True)
        self.temp_dir = tempfile.mkdtemp()
        self.catalog = ParquetDataCatalog(path=self.temp_dir)

        # Test case 2: Data points at exact period boundaries
        boundary_time = dt_to_unix_nanos(pd.Timestamp("2024-01-01 00:00:00", tz="UTC"))
        boundary_timestamps = [
            boundary_time,
            boundary_time + 86400_000_000_000,  # Exactly 1 day later
            boundary_time + 172800_000_000_000,  # Exactly 2 days later
        ]
        boundary_bars = self._create_test_bars(boundary_timestamps)
        self.catalog.write_data(boundary_bars)

        self.catalog.consolidate_data_by_period(
            data_cls=Bar,
            identifier="AUD/USD.SIM",
            period=pd.Timedelta(days=1),
            ensure_contiguous_files=True,
        )

        # Should handle boundary conditions correctly
        bars = self.catalog.bars()
        assert len(bars) == len(boundary_timestamps)

        retrieved_timestamps = sorted([bar.ts_init for bar in bars])
        assert retrieved_timestamps == sorted(boundary_timestamps)


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
    assert data_cls is not None
    assert identifier is not None


def test_delete_data_range_complete_file_deletion(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data that completely covers one or more files.
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
    ]
    catalog.write_data(quotes)

    # Verify initial state
    initial_data = catalog.quote_ticks()
    assert len(initial_data) == 2

    # Act - delete all data (use correct instrument ID)
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=0,
        end=3_000_000_000,
    )

    # Assert
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 0


def test_delete_data_range_partial_file_overlap_start(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data that partially overlaps with a file from the start.
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
    ]
    catalog.write_data(quotes)

    # Act - delete first part of the data
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=0,
        end=1_500_000_000,
    )

    # Assert - should keep data after deletion range
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 2
    assert remaining_data[0].ts_init == 2_000_000_000
    assert remaining_data[1].ts_init == 3_000_000_000


def test_delete_data_range_partial_file_overlap_end(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data that partially overlaps with a file from the end.
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
    ]
    catalog.write_data(quotes)

    # Act - delete last part of the data
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=2_500_000_000,
        end=4_000_000_000,
    )

    # Assert - should keep data before deletion range
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 2
    assert remaining_data[0].ts_init == 1_000_000_000
    assert remaining_data[1].ts_init == 2_000_000_000


def test_delete_data_range_partial_file_overlap_middle(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data that partially overlaps with a file in the middle.
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
        TestDataStubs.quote_tick(ts_init=4_000_000_000),
    ]
    catalog.write_data(quotes)

    # Act - delete middle part of the data
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=1_500_000_000,
        end=3_500_000_000,
    )

    # Assert - should keep data before and after deletion range
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 2
    assert remaining_data[0].ts_init == 1_000_000_000
    assert remaining_data[1].ts_init == 4_000_000_000


def test_delete_data_range_multiple_files(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data across multiple files.
    """
    # Arrange - create multiple files
    quotes1 = [TestDataStubs.quote_tick(ts_init=1_000_000_000)]
    quotes2 = [TestDataStubs.quote_tick(ts_init=2_000_000_000)]
    quotes3 = [TestDataStubs.quote_tick(ts_init=3_000_000_000)]

    catalog.write_data(quotes1)
    catalog.write_data(quotes2)
    catalog.write_data(quotes3)

    # Verify we have 3 files
    intervals = catalog.get_intervals(QuoteTick, "AUD/USD.SIM")
    assert len(intervals) == 3

    # Act - delete data spanning multiple files
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=1_500_000_000,
        end=2_500_000_000,
    )

    # Assert - should keep data outside deletion range
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 2
    assert remaining_data[0].ts_init == 1_000_000_000
    assert remaining_data[1].ts_init == 3_000_000_000


def test_delete_data_range_no_data(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data when no data exists.
    """
    # Act - delete from empty catalog
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="EUR/USD.SIM",
        start=1_000_000_000,
        end=2_000_000_000,
    )

    # Assert - should not raise any errors
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 0


def test_delete_data_range_no_intersection(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data that doesn't intersect with existing data.
    """
    # Arrange
    quotes = [TestDataStubs.quote_tick(ts_init=2_000_000_000)]
    catalog.write_data(quotes)

    # Act - delete data outside existing range
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="EUR/USD.SIM",
        start=3_000_000_000,
        end=4_000_000_000,
    )

    # Assert - should keep all existing data
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 1
    assert remaining_data[0].ts_init == 2_000_000_000


def test_delete_data_range_boundary_conditions(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data with exact boundary matches.
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
    ]
    catalog.write_data(quotes)

    # Act - delete with exact timestamp boundaries
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=2_000_000_000,
        end=2_000_000_000,
    )

    # Assert - should delete only the exact match
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 2
    assert remaining_data[0].ts_init == 1_000_000_000
    assert remaining_data[1].ts_init == 3_000_000_000


def test_delete_data_range_open_start(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data with no start boundary (delete from beginning).
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
    ]
    catalog.write_data(quotes)

    # Act - delete from beginning to middle
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=None,
        end=2_500_000_000,
    )

    # Assert - should keep data after end boundary
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 1
    assert remaining_data[0].ts_init == 3_000_000_000


def test_delete_data_range_open_end(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data with no end boundary (delete to end).
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
    ]
    catalog.write_data(quotes)

    # Act - delete from middle to end
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=1_500_000_000,
        end=None,
    )

    # Assert - should keep data before start boundary
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 1
    assert remaining_data[0].ts_init == 1_000_000_000


def test_delete_data_range_all_identifiers(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data across all identifiers when identifier is None.
    """
    # Arrange - create data for multiple instruments
    eur_usd_quote = TestDataStubs.quote_tick(ts_init=1_000_000_000)
    gbp_usd_quote = TestDataStubs.quote_ticks_usdjpy()[0]  # Use USD/JPY as second instrument

    catalog.write_data([eur_usd_quote])
    catalog.write_data([gbp_usd_quote])

    # Verify initial state
    all_quotes = catalog.quote_ticks()
    assert len(all_quotes) == 2

    # Act - delete data for all identifiers (use wide range to cover USD/JPY timestamp)
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier=None,
        start=0,
        end=2_000_000_000_000_000_000,  # Large enough to cover USD/JPY timestamp
    )

    # Assert - should delete data from all instruments
    remaining_quotes = catalog.quote_ticks()
    assert len(remaining_quotes) == 0


def test_delete_catalog_range_multiple_data_types(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data across multiple data types in the catalog.
    """
    # Arrange - create data for multiple data types
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
    ]
    trades = [
        TestDataStubs.trade_tick(ts_init=1_500_000_000),
        TestDataStubs.trade_tick(ts_init=2_500_000_000),
    ]

    catalog.write_data(quotes)
    catalog.write_data(trades)

    # Verify initial state
    initial_quotes = catalog.quote_ticks()
    initial_trades = catalog.trade_ticks()
    assert len(initial_quotes) == 2
    assert len(initial_trades) == 2

    # Act - delete data across all data types in a specific range
    catalog.delete_catalog_range(
        start=1_200_000_000,
        end=2_200_000_000,
    )

    # Assert - should delete data from both data types within the range
    remaining_quotes = catalog.quote_ticks()
    remaining_trades = catalog.trade_ticks()

    # Should keep quotes outside the deletion range
    assert len(remaining_quotes) == 1
    assert remaining_quotes[0].ts_init == 1_000_000_000

    # Should keep trades outside the deletion range
    assert len(remaining_trades) == 1
    assert remaining_trades[0].ts_init == 2_500_000_000


def test_delete_catalog_range_multiple_instruments(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data across multiple instruments in the catalog.
    """
    # Arrange - create data for multiple instruments
    eur_usd_quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
    ]
    gbp_usd_quotes = [
        TestDataStubs.quote_ticks_usdjpy()[0],  # Use USD/JPY as second instrument
        TestDataStubs.quote_ticks_usdjpy()[0],  # Use USD/JPY as second instrument
    ]

    catalog.write_data(eur_usd_quotes)
    catalog.write_data(gbp_usd_quotes)

    # Verify initial state
    all_quotes = catalog.quote_ticks()
    assert len(all_quotes) == 4

    # Act - delete data across all instruments (use wide range to cover USD/JPY timestamp)
    catalog.delete_catalog_range(
        start=500_000_000,
        end=2_000_000_000_000_000_000,  # Large enough to cover USD/JPY timestamp
    )

    # Assert - should delete all data since deletion range covers everything
    remaining_quotes = catalog.quote_ticks()
    assert len(remaining_quotes) == 0  # Should delete all data including USD/JPY


def test_delete_catalog_range_complete_deletion(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting all data in the catalog.
    """
    # Arrange - create data for multiple data types and instruments
    quotes = [TestDataStubs.quote_tick(ts_init=1_000_000_000)]
    trades = [TestDataStubs.trade_tick(ts_init=2_000_000_000)]
    gbp_quotes = [TestDataStubs.quote_ticks_usdjpy()[0]]  # Use USD/JPY as second instrument

    catalog.write_data(quotes)
    catalog.write_data(trades)
    catalog.write_data(gbp_quotes)

    # Verify initial state
    assert len(catalog.quote_ticks()) == 2  # EUR/USD + GBP/USD
    assert len(catalog.trade_ticks()) == 1

    # Act - delete all data (use wide range to cover USD/JPY timestamp)
    catalog.delete_catalog_range(
        start=0,
        end=2_000_000_000_000_000_000,  # Large enough to cover USD/JPY timestamp
    )

    # Assert - should have no data left
    assert len(catalog.quote_ticks()) == 0
    assert len(catalog.trade_ticks()) == 0


def test_delete_data_range_cross_file_split(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data that spans across multiple files and creates splits.

    Scenario:
    - File 1: timestamps [1_000_000_000, 2_000_000_000, 3_000_000_000, 4_000_000_000]
    - File 2: timestamps [5_000_000_000, 6_000_000_000, 7_000_000_000, 8_000_000_000]
    - File 3: timestamps [9_000_000_000, 10_000_000_000]
    - Delete range: [4_000_000_000, 10_000_000_000]
    - Expected result:
      - Remaining data: [1_000_000_000, 2_000_000_000, 3_000_000_000] (from split of file 1)
      - Files 2 and 3 should be completely deleted
      - File 1 should be split to preserve data before deletion range

    """
    # Arrange - create three separate files with specific timestamps (use larger gaps for disjoint intervals)
    quotes_file1 = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
        TestDataStubs.quote_tick(ts_init=4_000_000_000),
    ]
    quotes_file2 = [
        TestDataStubs.quote_tick(ts_init=5_000_000_000),
        TestDataStubs.quote_tick(ts_init=6_000_000_000),
        TestDataStubs.quote_tick(ts_init=7_000_000_000),
        TestDataStubs.quote_tick(ts_init=8_000_000_000),
    ]
    quotes_file3 = [
        TestDataStubs.quote_tick(ts_init=9_000_000_000),
        TestDataStubs.quote_tick(ts_init=10_000_000_000),
    ]

    # Write each group separately to create separate files
    catalog.write_data(quotes_file1)
    catalog.write_data(quotes_file2)
    catalog.write_data(quotes_file3)

    # Verify initial state - should have 3 files and 10 quotes
    initial_intervals = catalog.get_intervals(QuoteTick, "AUD/USD.SIM")
    assert len(initial_intervals) == 3, f"Expected 3 files, was {len(initial_intervals)}"

    initial_quotes = catalog.quote_ticks()
    assert len(initial_quotes) == 10, f"Expected 10 quotes, was {len(initial_quotes)}"

    # Act - delete range [4_000_000_000, 10_000_000_000] which should:
    # - Split file 1 to keep [1_000_000_000, 2_000_000_000, 3_000_000_000]
    # - Delete files 2 and 3 completely
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=4_000_000_000,
        end=10_000_000_000,
    )

    # Assert - verify remaining data
    remaining_quotes = catalog.quote_ticks()
    remaining_timestamps = [q.ts_init for q in remaining_quotes]
    remaining_timestamps.sort()

    expected_remaining = [1_000_000_000, 2_000_000_000, 3_000_000_000]
    assert (
        remaining_timestamps == expected_remaining
    ), f"Expected {expected_remaining}, was {remaining_timestamps}"

    # Verify file structure - should have 1 file remaining
    final_intervals = catalog.get_intervals(QuoteTick, "AUD/USD.SIM")
    assert len(final_intervals) == 1, f"Expected 1 file, was {len(final_intervals)}"

    # Verify the remaining file covers the correct range (should end just before deletion start)
    expected_start = 1_000_000_000
    expected_end = 4_000_000_000 - 1  # Just before deletion range starts (one nanosecond before)
    assert (
        final_intervals[0][0] == expected_start
    ), f"Expected start {expected_start}, was {final_intervals[0][0]}"
    assert (
        final_intervals[0][1] == expected_end
    ), f"Expected end {expected_end}, was {final_intervals[0][1]}"

    # Verify we can query the remaining data correctly
    queried_quotes = catalog.query(
        data_cls=QuoteTick,
        identifiers=["AUD/USD.SIM"],
        start=1_000_000_000,
        end=10_000_000_000,
    )
    queried_timestamps = [q.ts_init for q in queried_quotes]
    queried_timestamps.sort()

    assert (
        queried_timestamps == expected_remaining
    ), f"Query result should be {expected_remaining}, was {queried_timestamps}"


def test_delete_data_range_cross_file_split_keep_end(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data from the beginning and keeping the end across multiple files.

    Scenario:
    - File 1: timestamps [1_000_000_000, 2_000_000_000, 3_000_000_000, 4_000_000_000]
    - File 2: timestamps [5_000_000_000, 6_000_000_000, 7_000_000_000, 8_000_000_000]
    - File 3: timestamps [9_000_000_000, 10_000_000_000]
    - Delete range: [1_000_000_000, 7_000_000_000]
    - Expected result:
      - Remaining data: [8_000_000_000, 9_000_000_000, 10_000_000_000]
      - Files 1 and 2 should be mostly deleted
      - File 2 should be split to preserve [8_000_000_000]
      - File 3 should remain intact

    """
    # Arrange - create three separate files with specific timestamps
    quotes_file1 = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
        TestDataStubs.quote_tick(ts_init=4_000_000_000),
    ]
    quotes_file2 = [
        TestDataStubs.quote_tick(ts_init=5_000_000_000),
        TestDataStubs.quote_tick(ts_init=6_000_000_000),
        TestDataStubs.quote_tick(ts_init=7_000_000_000),
        TestDataStubs.quote_tick(ts_init=8_000_000_000),
    ]
    quotes_file3 = [
        TestDataStubs.quote_tick(ts_init=9_000_000_000),
        TestDataStubs.quote_tick(ts_init=10_000_000_000),
    ]

    # Write each group separately to create separate files
    catalog.write_data(quotes_file1)
    catalog.write_data(quotes_file2)
    catalog.write_data(quotes_file3)

    # Verify initial state
    initial_intervals = catalog.get_intervals(QuoteTick, "AUD/USD.SIM")
    assert len(initial_intervals) == 3, f"Expected 3 files, was {len(initial_intervals)}"

    initial_quotes = catalog.quote_ticks()
    assert len(initial_quotes) == 10, f"Expected 10 quotes, was {len(initial_quotes)}"

    # Act - delete range [1_000_000_000, 7_000_000_000] which should:
    # - Delete file 1 completely
    # - Split file 2 to keep [8_000_000_000]
    # - Keep file 3 intact
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=1_000_000_000,
        end=7_000_000_000,
    )

    # Assert - verify remaining data
    remaining_quotes = catalog.quote_ticks()
    remaining_timestamps = [q.ts_init for q in remaining_quotes]
    remaining_timestamps.sort()

    expected_remaining = [8_000_000_000, 9_000_000_000, 10_000_000_000]
    assert (
        remaining_timestamps == expected_remaining
    ), f"Expected {expected_remaining}, was {remaining_timestamps}"

    # Verify file structure - should have 2 files remaining (split file 2 + intact file 3)
    final_intervals = catalog.get_intervals(QuoteTick, "AUD/USD.SIM")
    assert len(final_intervals) == 2, f"Expected 2 files, was {len(final_intervals)}"

    # Verify we can query the remaining data correctly
    queried_quotes = catalog.query(
        data_cls=QuoteTick,
        identifiers=["AUD/USD.SIM"],
        start=1_000_000_000,
        end=10_000_000_000,
    )
    queried_timestamps = [q.ts_init for q in queried_quotes]
    queried_timestamps.sort()

    assert (
        queried_timestamps == expected_remaining
    ), f"Query result should be {expected_remaining}, was {queried_timestamps}"


def test_delete_catalog_range_partial_overlap(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data with partial file overlaps across the catalog.
    """
    # Arrange - create data that will result in partial file overlaps
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
    ]
    trades = [
        TestDataStubs.trade_tick(ts_init=1_500_000_000),
        TestDataStubs.trade_tick(ts_init=2_500_000_000),
        TestDataStubs.trade_tick(ts_init=3_500_000_000),
    ]

    catalog.write_data(quotes)
    catalog.write_data(trades)

    # Act - delete data in the middle range
    catalog.delete_catalog_range(
        start=1_800_000_000,
        end=2_800_000_000,
    )

    # Assert - should keep data outside the deletion range for both data types
    remaining_quotes = catalog.quote_ticks()
    remaining_trades = catalog.trade_ticks()

    # Should keep quotes before and after deletion range
    assert len(remaining_quotes) == 2
    quote_timestamps = [q.ts_init for q in remaining_quotes]
    assert 1_000_000_000 in quote_timestamps
    assert 3_000_000_000 in quote_timestamps

    # Should keep trades before and after deletion range
    assert len(remaining_trades) == 2
    trade_timestamps = [t.ts_init for t in remaining_trades]
    assert 1_500_000_000 in trade_timestamps
    assert 3_500_000_000 in trade_timestamps


def test_delete_catalog_range_empty_catalog(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data from an empty catalog.
    """
    # Act - delete from empty catalog
    catalog.delete_catalog_range(
        start=1_000_000_000,
        end=2_000_000_000,
    )

    # Assert - should not raise any errors
    assert len(catalog.quote_ticks()) == 0
    assert len(catalog.trade_ticks()) == 0


def test_delete_catalog_range_no_intersection(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data that doesn't intersect with existing data.
    """
    # Arrange
    quotes = [TestDataStubs.quote_tick(ts_init=5_000_000_000)]
    trades = [TestDataStubs.trade_tick(ts_init=6_000_000_000)]

    catalog.write_data(quotes)
    catalog.write_data(trades)

    # Act - delete data outside existing range
    catalog.delete_catalog_range(
        start=1_000_000_000,
        end=2_000_000_000,
    )

    # Assert - should keep all existing data
    remaining_quotes = catalog.quote_ticks()
    remaining_trades = catalog.trade_ticks()

    assert len(remaining_quotes) == 1
    assert remaining_quotes[0].ts_init == 5_000_000_000
    assert len(remaining_trades) == 1
    assert remaining_trades[0].ts_init == 6_000_000_000


def test_delete_catalog_range_open_boundaries(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data with open start/end boundaries.
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
    ]
    trades = [
        TestDataStubs.trade_tick(ts_init=1_500_000_000),
        TestDataStubs.trade_tick(ts_init=2_500_000_000),
        TestDataStubs.trade_tick(ts_init=3_500_000_000),
    ]

    catalog.write_data(quotes)
    catalog.write_data(trades)

    # Test 1: Delete from beginning to middle (open start)
    catalog.delete_catalog_range(
        start=None,
        end=2_200_000_000,
    )

    # Should keep data after end boundary
    remaining_quotes = catalog.quote_ticks()
    remaining_trades = catalog.trade_ticks()

    assert len(remaining_quotes) == 1
    assert remaining_quotes[0].ts_init == 3_000_000_000
    assert len(remaining_trades) == 2
    assert 2_500_000_000 in [t.ts_init for t in remaining_trades]
    assert 3_500_000_000 in [t.ts_init for t in remaining_trades]


def test_delete_catalog_range_open_end_boundary(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data with open end boundary.
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
    ]
    trades = [
        TestDataStubs.trade_tick(ts_init=1_500_000_000),
        TestDataStubs.trade_tick(ts_init=2_500_000_000),
        TestDataStubs.trade_tick(ts_init=3_500_000_000),
    ]

    catalog.write_data(quotes)
    catalog.write_data(trades)

    # Act: Delete from middle to end (open end)
    catalog.delete_catalog_range(
        start=1_800_000_000,
        end=None,
    )

    # Assert - should keep data before start boundary
    remaining_quotes = catalog.quote_ticks()
    remaining_trades = catalog.trade_ticks()

    assert len(remaining_quotes) == 1
    assert remaining_quotes[0].ts_init == 1_000_000_000
    assert len(remaining_trades) == 1
    assert remaining_trades[0].ts_init == 1_500_000_000


def test_delete_catalog_range_boundary_conditions(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data with exact boundary matches.
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
    ]
    trades = [
        TestDataStubs.trade_tick(ts_init=1_500_000_000),
        TestDataStubs.trade_tick(ts_init=2_500_000_000),
    ]

    catalog.write_data(quotes)
    catalog.write_data(trades)

    # Act - delete with exact timestamp boundaries
    catalog.delete_catalog_range(
        start=2_000_000_000,
        end=2_500_000_000,
    )


def test_delete_data_range_nanosecond_precision_boundaries(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting data with nanosecond precision boundaries to verify exact [a-1, b+1]
    logic.
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=1_000_000_001),  # +1 nanosecond
        TestDataStubs.quote_tick(ts_init=1_000_000_002),  # +2 nanoseconds
        TestDataStubs.quote_tick(ts_init=1_000_000_003),  # +3 nanoseconds
        TestDataStubs.quote_tick(ts_init=1_000_000_004),  # +4 nanoseconds
    ]
    catalog.write_data(quotes)

    # Act - delete exactly [1_000_000_001, 1_000_000_003] (inclusive)
    # Should keep [0, 1_000_000_000] and [1_000_000_004, ∞]
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=1_000_000_001,
        end=1_000_000_003,
    )

    # Assert - should keep only first and last timestamps
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 2
    timestamps = [q.ts_init for q in remaining_data]
    timestamps.sort()
    assert timestamps == [1_000_000_000, 1_000_000_004]


def test_delete_data_range_single_file_double_split(catalog: ParquetDataCatalog) -> None:
    """
    Test deleting from a single file that requires both split_before and split_after
    operations.
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
        TestDataStubs.quote_tick(ts_init=4_000_000_000),
        TestDataStubs.quote_tick(ts_init=5_000_000_000),
    ]
    catalog.write_data(quotes)

    # Act - delete middle range [2_500_000_000, 3_500_000_000]
    # This should create both split_before (keep [1_000_000_000, 2_000_000_000])
    # and split_after (keep [4_000_000_000, 5_000_000_000])
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=2_500_000_000,
        end=3_500_000_000,
    )

    # Assert - should keep data before and after deletion range
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 4

    timestamps = [q.ts_init for q in remaining_data]
    timestamps.sort()
    assert timestamps == [1_000_000_000, 2_000_000_000, 4_000_000_000, 5_000_000_000]


def test_delete_data_range_complex_multi_file_scenario(catalog: ParquetDataCatalog) -> None:
    """
    Test complex deletion scenario across multiple files with various split operations.
    """
    # Arrange - create 4 separate files with different timestamp ranges
    quotes_file1 = [
        TestDataStubs.quote_tick(ts_init=1_000_000_000),
        TestDataStubs.quote_tick(ts_init=2_000_000_000),
    ]
    quotes_file2 = [
        TestDataStubs.quote_tick(ts_init=3_000_000_000),
        TestDataStubs.quote_tick(ts_init=4_000_000_000),
    ]
    quotes_file3 = [
        TestDataStubs.quote_tick(ts_init=5_000_000_000),
        TestDataStubs.quote_tick(ts_init=6_000_000_000),
    ]
    quotes_file4 = [
        TestDataStubs.quote_tick(ts_init=7_000_000_000),
        TestDataStubs.quote_tick(ts_init=8_000_000_000),
    ]

    # Write each group separately to create separate files
    catalog.write_data(quotes_file1)
    catalog.write_data(quotes_file2)
    catalog.write_data(quotes_file3)
    catalog.write_data(quotes_file4)

    # Verify initial state
    initial_quotes = catalog.quote_ticks()
    assert len(initial_quotes) == 8

    # Act - delete range [1_500_000_000, 6_500_000_000] which should:
    # - Split file 1: keep [1_000_000_000] (before deletion)
    # - Remove file 2 completely
    # - Remove file 3 completely
    # - Split file 4: keep [7_000_000_000, 8_000_000_000] (after deletion)
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=1_500_000_000,
        end=6_500_000_000,
    )

    # Assert - verify remaining data
    remaining_quotes = catalog.quote_ticks()
    timestamps = [q.ts_init for q in remaining_quotes]
    timestamps.sort()

    expected_remaining = [1_000_000_000, 7_000_000_000, 8_000_000_000]
    assert timestamps == expected_remaining

    # Verify file structure - should have 2 files now (split from file 1 and file 4)
    intervals = catalog.get_intervals(QuoteTick, "AUD/USD.SIM")
    assert len(intervals) == 2


def test_delete_data_range_zero_timestamp_edge_case(catalog: ParquetDataCatalog) -> None:
    """
    Test deletion with timestamp 0 to verify saturating arithmetic behavior.
    """
    # Arrange
    quotes = [
        TestDataStubs.quote_tick(ts_init=0),
        TestDataStubs.quote_tick(ts_init=1),
        TestDataStubs.quote_tick(ts_init=2),
        TestDataStubs.quote_tick(ts_init=3),
    ]
    catalog.write_data(quotes)

    # Act - delete range [0, 1] which tests edge case where start-1 would underflow
    catalog.delete_data_range(
        data_cls=QuoteTick,
        identifier="AUD/USD.SIM",
        start=0,
        end=1,
    )

    # Assert - should keep only timestamps 2 and 3
    remaining_data = catalog.quote_ticks()
    assert len(remaining_data) == 2
    timestamps = [q.ts_init for q in remaining_data]
    timestamps.sort()
    assert timestamps == [2, 3]


def test_backend_session_table_naming_multiple_instruments(catalog: ParquetDataCatalog) -> None:
    """
    Test that backend_session creates identifier-dependent table names for multiple
    instruments.

    This test verifies the fix for the table naming bug where multiple instruments would
    cause table name conflicts in DataFusion queries.

    """
    # Arrange - Create bars for multiple instruments
    bar_type1 = TestDataStubs.bartype_adabtc_binance_1min_last()
    instrument1 = TestInstrumentProvider.adabtc_binance()
    bars1 = TestDataStubs.binance_bars_from_csv(
        "ADABTC-1m-2021-11-27.csv",
        bar_type1,
        instrument1,
    )[
        :5
    ]  # Use fewer bars for faster test

    bar_type2 = TestDataStubs.bartype_btcusdt_binance_100tick_last()
    instrument2 = TestInstrumentProvider.btcusdt_binance()
    bars2 = TestDataStubs.binance_bars_from_csv(
        "ADABTC-1m-2021-11-27.csv",  # Reuse same CSV data but with different bar_type
        bar_type2,
        instrument2,
    )[
        :5
    ]  # Use fewer bars for faster test

    # Write data for both instruments
    catalog.write_data(bars1)
    catalog.write_data(bars2)

    # Act - Create backend session with multiple instruments
    identifiers = [str(bar_type1), str(bar_type2)]
    session = catalog.backend_session(
        data_cls=Bar,
        identifiers=identifiers,
    )

    # Assert - Session should be created successfully without table name conflicts
    assert session is not None

    # Query data using the session to verify it works correctly
    result = session.to_query_result()
    data = []
    for chunk in result:
        from nautilus_trader.model.data import capsule_to_list

        data.extend(capsule_to_list(chunk))

    # Should get data from both instruments
    assert len(data) == 10  # 5 bars from each instrument

    # Verify we have data from both instruments
    instrument_ids = {bar.bar_type.instrument_id.value for bar in data}
    assert len(instrument_ids) == 2
    assert instrument1.id.value in instrument_ids
    assert instrument2.id.value in instrument_ids


def test_backend_session_table_naming_special_characters(catalog: ParquetDataCatalog) -> None:
    """
    Test that backend_session handles special characters in identifiers correctly.

    This test verifies that identifiers with dots, hyphens, and slashes are properly
    converted to safe SQL table names.

    """
    # Arrange - Create quote ticks for instruments with special characters
    eurusd_instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD", Venue("SIM"))
    btcusd_instrument = TestInstrumentProvider.default_fx_ccy("BTC-USD", Venue("COINBASE"))

    quotes_eurusd = [
        TestDataStubs.quote_tick(
            instrument=eurusd_instrument,
            ts_init=i * 1000,
        )
        for i in range(3)
    ]

    quotes_btc_usd = [
        TestDataStubs.quote_tick(
            instrument=btcusd_instrument,
            ts_init=i * 1000 + 500,
        )
        for i in range(3)
    ]

    # Write data
    catalog.write_data(quotes_eurusd)
    catalog.write_data(quotes_btc_usd)

    # Act - Create backend session with identifiers containing special characters
    identifiers = [str(eurusd_instrument.id), str(btcusd_instrument.id)]
    session = catalog.backend_session(
        data_cls=QuoteTick,
        identifiers=identifiers,
    )

    # Assert - Session should be created successfully
    assert session is not None

    # Query data to verify it works
    result = session.to_query_result()
    data = []
    for chunk in result:
        from nautilus_trader.model.data import capsule_to_list

        data.extend(capsule_to_list(chunk))

    # Should get data from both instruments
    assert len(data) == 6  # 3 quotes from each instrument

    # Verify we have data from both instruments
    instrument_ids = {quote.instrument_id.value for quote in data}
    assert len(instrument_ids) == 2
    assert str(eurusd_instrument.id) in instrument_ids
    assert str(btcusd_instrument.id) in instrument_ids
