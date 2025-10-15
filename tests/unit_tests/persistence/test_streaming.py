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

import copy
from collections import Counter

from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.backtest.results import BacktestResult
from nautilus_trader.common.signal import generate_signal_class
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import NautilusKernelConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.rust.model import BookType
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.stubs.persistence import TestPersistenceStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


class TestPersistenceStreaming:
    def setup(self) -> None:
        self.catalog: ParquetDataCatalog | None = None

    def _run_default_backtest(
        self,
        catalog_betfair: ParquetDataCatalog,
        book_type: str = "L1_MBP",
    ) -> list[BacktestResult]:
        self.catalog = catalog_betfair
        instrument = self.catalog.instruments()[0]
        run_config = BetfairTestStubs.backtest_run_config(
            catalog_path=catalog_betfair.path,
            catalog_fs_protocol="file",
            instrument_id=instrument.id,
            flush_interval_ms=5_000,
            bypass_logging=True,
            book_type=book_type,
        )

        node = BacktestNode(configs=[run_config])

        # Act
        backtest_result = node.run()

        return backtest_result

    def test_feather_writer(self, catalog_betfair: ParquetDataCatalog) -> None:
        # Arrange
        backtest_result = self._run_default_backtest(catalog_betfair)
        instance_id = backtest_result[0].instance_id

        # Assert
        result = catalog_betfair.read_backtest(
            instance_id=instance_id,
            raise_on_failed_deserialize=True,
        )
        result = dict(Counter([r.__class__.__name__ for r in result]))  # type: ignore [assignment]

        # TODO: Backtest needs to be reconfigured to use either deltas or trades
        expected = {
            "AccountState": 387,
            "BettingInstrument": 1,
            "ComponentStateChanged": 34,
            "OrderAccepted": 192,
            "OrderBookDelta": 1307,
            "OrderCanceled": 100,
            "OrderFilled": 188,  # Doubled due to publishing to both events.order.* and events.fills.*
            "OrderInitialized": 193,
            "OrderSubmitted": 193,
            "PositionChanged": 90,
            "PositionClosed": 3,
            "PositionOpened": 3,
            "TradeTick": 179,
        }

        assert result == expected

    def test_feather_writer_custom_data(
        self,
        catalog_betfair: ParquetDataCatalog,
    ) -> None:
        # Arrange
        self.catalog = catalog_betfair
        TestPersistenceStubs.setup_news_event_persistence()

        # Load news events into catalog
        news_events = TestPersistenceStubs.news_events()
        self.catalog.write_data(news_events)

        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            data_cls=NewsEventData.fully_qualified_name(),
            client_id="NewsClient",
        )

        # Add some arbitrary instrument data to appease BacktestEngine
        instrument_data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            data_cls=InstrumentStatus.fully_qualified_name(),
        )

        streaming = BetfairTestStubs.streaming_config(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
        )

        run_config = BacktestRunConfig(
            engine=BacktestEngineConfig(streaming=streaming),
            data=[data_config, instrument_data_config],
            venues=[BetfairTestStubs.betfair_venue_config(book_type="L1_MBP")],
            chunk_size=None,  # No streaming
        )

        # Act
        node = BacktestNode(configs=[run_config])
        r = node.run()

        # Assert
        result = self.catalog.read_backtest(
            instance_id=r[0].instance_id,
            raise_on_failed_deserialize=True,
        )

        result_counter = Counter([r.__class__.__name__ for r in result])
        assert result_counter["NewsEventData"] == 5_000  # Reduced from 86_985 for faster testing

    def test_feather_writer_include_types(
        self,
        catalog_betfair: ParquetDataCatalog,
    ) -> None:
        # Arrange
        self.catalog = catalog_betfair
        TestPersistenceStubs.setup_news_event_persistence()

        # Load news events into catalog
        news_events = TestPersistenceStubs.news_events()
        self.catalog.write_data(news_events)

        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            data_cls=NewsEventData.fully_qualified_name(),
            client_id="NewsClient",
        )

        # Add some arbitrary instrument data to appease BacktestEngine
        instrument_data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            data_cls=InstrumentStatus.fully_qualified_name(),
        )

        streaming = BetfairTestStubs.streaming_config(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            include_types=[NewsEventData],
        )

        run_config = BacktestRunConfig(
            engine=BacktestEngineConfig(streaming=streaming),
            data=[data_config, instrument_data_config],
            venues=[BetfairTestStubs.betfair_venue_config(book_type="L1_MBP")],
            chunk_size=None,  # No streaming
        )

        # Act
        node = BacktestNode(configs=[run_config])
        r = node.run()

        # Assert
        result = self.catalog.read_backtest(
            instance_id=r[0].instance_id,
            raise_on_failed_deserialize=True,
        )

        result_counter = Counter([r.__class__.__name__ for r in result])
        assert result_counter["NewsEventData"] == 5_000  # Reduced from 86_985 for faster testing
        assert len(result_counter) == 1

    def test_feather_writer_stream_to_data(
        self,
        catalog_betfair: ParquetDataCatalog,
    ) -> None:
        # Arrange
        self.catalog = catalog_betfair
        TestPersistenceStubs.setup_news_event_persistence()

        # Load news events into catalog
        news_events = TestPersistenceStubs.news_events()
        self.catalog.write_data(news_events)

        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            data_cls=NewsEventData.fully_qualified_name(),
            client_id="NewsClient",
        )

        # Add some arbitrary instrument data to appease BacktestEngine
        instrument_data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            data_cls=InstrumentStatus.fully_qualified_name(),
        )

        streaming = BetfairTestStubs.streaming_config(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
        )

        run_config = BacktestRunConfig(
            engine=BacktestEngineConfig(streaming=streaming),
            data=[data_config, instrument_data_config],
            venues=[BetfairTestStubs.betfair_venue_config(book_type="L1_MBP")],
            chunk_size=None,  # No streaming
        )

        node = BacktestNode(configs=[run_config])
        r = node.run()

        # Act
        # NewsEventData is overridden here with data from the stream, but it should be the same data
        self.catalog.convert_stream_to_data(r[0].instance_id, NewsEventData)

        node2 = BacktestNode(configs=[run_config])
        r2 = node2.run()

        # Assert
        result = self.catalog.read_backtest(
            instance_id=r2[0].instance_id,
            raise_on_failed_deserialize=True,
        )

        result_counter = Counter([r.__class__.__name__ for r in result])
        assert result_counter["NewsEventData"] == 5_000  # Reduced from 86_985 for faster testing

    def test_stream_to_data_directory(self, catalog_betfair: ParquetDataCatalog):
        # Arrange - run backtest then delete data so we can test against it
        [backtest_result] = self._run_default_backtest(catalog_betfair)
        catalog_betfair.fs.rm(f"{catalog_betfair.path}/data", recursive=True)
        assert not catalog_betfair.list_data_types()

        # Act
        catalog_betfair.convert_stream_to_data(
            backtest_result.instance_id,
            subdirectory="backtest",
            data_cls=TradeTick,
        )

        # Assert
        assert catalog_betfair.list_data_types() == ["trade_tick"]

    def test_feather_writer_signal_data(
        self,
        catalog_betfair: ParquetDataCatalog,
    ) -> None:
        # Arrange
        self.catalog = catalog_betfair
        instrument_id = self.catalog.instruments()[0].id
        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            data_cls=TradeTick,
        )

        streaming = BetfairTestStubs.streaming_config(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
        )
        run_config = BacktestRunConfig(
            engine=BacktestEngineConfig(
                streaming=streaming,
                strategies=[
                    ImportableStrategyConfig(
                        strategy_path="nautilus_trader.examples.strategies.signal_strategy:SignalStrategy",
                        config_path="nautilus_trader.examples.strategies.signal_strategy:SignalStrategyConfig",
                        config={"instrument_id": instrument_id.value},
                    ),
                ],
            ),
            data=[data_config],
            venues=[BetfairTestStubs.betfair_venue_config(book_type="L1_MBP")],
            chunk_size=None,  # No streaming
        )

        # Act
        node = BacktestNode(configs=[run_config])
        r = node.run()

        # Assert
        result = self.catalog.read_backtest(
            instance_id=r[0].instance_id,
            raise_on_failed_deserialize=True,
        )

        result = Counter([r.__class__.__name__ for r in result])  # type: ignore
        assert result["SignalCounter"] == 179  # type: ignore

    def test_generate_signal_class(self) -> None:
        # Arrange
        cls = generate_signal_class(name="test", value_type=float)

        # Act
        instance = cls(value=5.0, ts_event=0, ts_init=0)

        # Assert
        assert isinstance(instance, Data)
        assert instance.ts_event == 0
        assert instance.value == 5.0
        assert instance.ts_init == 0

    def test_config_write(
        self,
        catalog_betfair: ParquetDataCatalog,
    ) -> None:
        # Arrange
        self.catalog = catalog_betfair
        instrument_id = self.catalog.instruments()[0].id
        streaming = BetfairTestStubs.streaming_config(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
        )
        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="file",
            data_cls=TradeTick,
        )

        run_config = BacktestRunConfig(
            engine=BacktestEngineConfig(
                streaming=streaming,
                strategies=[
                    ImportableStrategyConfig(
                        strategy_path="nautilus_trader.examples.strategies.signal_strategy:SignalStrategy",
                        config_path="nautilus_trader.examples.strategies.signal_strategy:SignalStrategyConfig",
                        config={"instrument_id": instrument_id.value},
                    ),
                ],
            ),
            data=[data_config],
            venues=[BetfairTestStubs.betfair_venue_config(book_type="L1_MBP")],
            chunk_size=None,  # No streaming
        )

        # Act
        node = BacktestNode(configs=[run_config])
        r = node.run()

        # Assert
        config_file = f"{self.catalog.path}/backtest/{r[0].instance_id}/config.json"
        assert self.catalog.fs.exists(config_file)
        raw = self.catalog.fs.open(config_file, "rb").read()
        assert isinstance(raw, bytes)
        assert NautilusKernelConfig.parse(raw)

    def test_feather_reader_returns_cython_objects(
        self,
        catalog_betfair: ParquetDataCatalog,
    ) -> None:
        # Arrange
        backtest_result = self._run_default_backtest(catalog_betfair)
        instance_id = backtest_result[0].instance_id

        # Act
        assert self.catalog
        result = self.catalog.read_backtest(
            instance_id=instance_id,
            raise_on_failed_deserialize=True,
        )

        # Assert
        assert len([d for d in result if d.__class__.__name__ == "TradeTick"]) == 179
        assert len([d for d in result if d.__class__.__name__ == "OrderBookDelta"]) == 1307

    def test_feather_reader_order_book_deltas(
        self,
        catalog_betfair: ParquetDataCatalog,
    ) -> None:
        # Arrange
        backtest_result = self._run_default_backtest(catalog_betfair)
        book = OrderBook(
            instrument_id=InstrumentId.from_str("1.166564490-237491-0.0.BETFAIR"),
            book_type=BookType.L2_MBP,
        )

        # Act
        assert self.catalog
        result = self.catalog.read_backtest(
            instance_id=backtest_result[0].instance_id,
            raise_on_failed_deserialize=True,
        )

        updates = [d for d in result if isinstance(d, OrderBookDelta)]

        # Assert
        for update in updates[:10]:
            book.apply_delta(update)
            copy.deepcopy(book)

    def test_read_backtest(
        self,
        catalog_betfair: ParquetDataCatalog,
    ) -> None:
        # Arrange
        [backtest_result] = self._run_default_backtest(catalog_betfair)

        # Act
        data = catalog_betfair.read_backtest(backtest_result.instance_id)
        counts = dict(Counter([d.__class__.__name__ for d in data]))

        # Assert
        expected = {
            "AccountState": 387,
            "BettingInstrument": 1,
            "ComponentStateChanged": 34,
            "OrderAccepted": 192,
            "OrderBookDelta": 1307,
            "OrderCanceled": 100,
            "OrderFilled": 188,  # Doubled due to publishing to both events.order.* and events.fills.*
            "OrderInitialized": 193,
            "OrderSubmitted": 193,
            "PositionChanged": 90,
            "PositionClosed": 3,
            "PositionOpened": 3,
            "TradeTick": 179,
        }
        assert counts == expected

    def test_feather_writer_per_bar_type(
        self,
        catalog_betfair: ParquetDataCatalog,
    ) -> None:
        # Arrange
        from nautilus_trader.cache.cache import Cache
        from nautilus_trader.common.component import TestClock
        from nautilus_trader.model.data import Bar
        from nautilus_trader.persistence.writer import StreamingFeatherWriter
        from nautilus_trader.test_kit.providers import TestInstrumentProvider
        from nautilus_trader.test_kit.stubs.data import TestDataStubs

        self.catalog = catalog_betfair

        # Create test infrastructure
        clock = TestClock()
        cache = Cache()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        cache.add_instrument(instrument)

        # Create writer with Bar in include_types
        writer = StreamingFeatherWriter(
            path=f"{self.catalog.path}/backtest/test_instance",
            cache=cache,
            clock=clock,
            fs_protocol="file",
            include_types=[Bar],
        )

        # Create bars with different bar types
        bar1 = TestDataStubs.bar_5decimal(ts_event=1000, ts_init=1000)
        bar2 = TestDataStubs.bar_5decimal_5min_bid()

        # Act - write bars
        writer.write(bar1)
        writer.write(bar2)
        writer.close()

        # Assert - check that bars were written to per-bar-type subdirectories
        bar_dir = f"{self.catalog.path}/backtest/test_instance/bar"

        # Verify directory structure exists
        assert self.catalog.fs.isdir(bar_dir)

        # Verify subdirectories for each bar type exist
        subdirs = [d for d in self.catalog.fs.glob(f"{bar_dir}/*") if self.catalog.fs.isdir(d)]
        assert len(subdirs) == 2  # One for each bar type

        # Verify feather files exist in subdirectories
        feather_files = list(self.catalog.fs.glob(f"{bar_dir}/**/*.feather"))
        assert len(feather_files) == 2  # One file per bar type

    def test_convert_stream_to_data_with_identifiers(
        self,
        catalog_betfair: ParquetDataCatalog,
    ) -> None:
        # Arrange
        from nautilus_trader.cache.cache import Cache
        from nautilus_trader.common.component import TestClock
        from nautilus_trader.model.data import Bar
        from nautilus_trader.persistence.writer import StreamingFeatherWriter
        from nautilus_trader.test_kit.providers import TestInstrumentProvider
        from nautilus_trader.test_kit.stubs.data import TestDataStubs

        self.catalog = catalog_betfair

        # Create test infrastructure
        clock = TestClock()
        cache = Cache()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        cache.add_instrument(instrument)

        # Create writer
        instance_id = "test_instance_identifiers"
        writer = StreamingFeatherWriter(
            path=f"{self.catalog.path}/backtest/{instance_id}",
            cache=cache,
            clock=clock,
            fs_protocol="file",
            include_types=[Bar],
        )

        # Create bars with different bar types (1-MINUTE and 5-MINUTE)
        bar1 = TestDataStubs.bar_5decimal(ts_event=1000, ts_init=1000)
        bar2 = TestDataStubs.bar_5decimal_5min_bid()

        # Write bars
        writer.write(bar1)
        writer.write(bar2)
        writer.close()

        # Act - convert only bars with "5-MINUTE" in their identifier
        self.catalog.convert_stream_to_data(
            instance_id,
            Bar,
            identifiers=["5-MINUTE"],
        )

        # Assert - verify only 5-MINUTE bars were written to catalog
        # Query all bars from catalog
        all_bars = self.catalog.bars()
        assert len(all_bars) == 1

        # Verify the bar is a 5-MINUTE bar
        assert "5-MINUTE" in str(all_bars[0].bar_type)

    def test_convert_stream_to_data_internal_to_external(
        self,
        catalog_betfair: ParquetDataCatalog,
    ) -> None:
        # Arrange
        from nautilus_trader.cache.cache import Cache
        from nautilus_trader.common.component import TestClock
        from nautilus_trader.model.data import Bar
        from nautilus_trader.model.data import BarSpecification
        from nautilus_trader.model.data import BarType
        from nautilus_trader.model.enums import AggregationSource
        from nautilus_trader.model.enums import BarAggregation
        from nautilus_trader.model.enums import PriceType
        from nautilus_trader.model.objects import Price
        from nautilus_trader.model.objects import Quantity
        from nautilus_trader.persistence.writer import StreamingFeatherWriter
        from nautilus_trader.test_kit.providers import TestInstrumentProvider

        self.catalog = catalog_betfair

        # Create test infrastructure
        clock = TestClock()
        cache = Cache()
        instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        cache.add_instrument(instrument)

        # Create writer
        instance_id = "test_instance_internal"
        writer = StreamingFeatherWriter(
            path=f"{self.catalog.path}/backtest/{instance_id}",
            cache=cache,
            clock=clock,
            fs_protocol="file",
            include_types=[Bar],
        )

        # Create a bar with INTERNAL aggregation source
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)
        bar_type_internal = BarType(
            instrument.id,
            bar_spec,
            AggregationSource.INTERNAL,
        )

        bar = Bar(
            bar_type=bar_type_internal,
            open=Price.from_str("1.00002"),
            high=Price.from_str("1.00004"),
            low=Price.from_str("1.00001"),
            close=Price.from_str("1.00003"),
            volume=Quantity.from_int(1_000_000),
            ts_event=1000,
            ts_init=1000,
        )

        # Write bar
        writer.write(bar)
        writer.close()

        # Act - convert stream to data (should convert INTERNAL to EXTERNAL)
        self.catalog.convert_stream_to_data(
            instance_id,
            Bar,
        )

        # Assert - verify bars were converted to EXTERNAL
        # Load all bars from catalog
        bars = self.catalog.bars()
        assert len(bars) == 1

        # Check that the bar has EXTERNAL aggregation source
        assert bars[0].bar_type.aggregation_source == AggregationSource.EXTERNAL
        assert str(bars[0].bar_type).endswith("-EXTERNAL")
