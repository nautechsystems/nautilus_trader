# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import fsspec
import pandas as pd
import pytest

from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.persistence.catalog.parquet.core import ParquetDataCatalog
from nautilus_trader.persistence.funcs import parse_bytes
from nautilus_trader.persistence.streaming.batching import generate_batches_rust
from nautilus_trader.persistence.streaming.engine import StreamingEngine
from nautilus_trader.persistence.streaming.engine import _BufferIterator
from nautilus_trader.persistence.streaming.engine import _StreamingBuffer
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from tests import TEST_DATA_DIR
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


class TestBatchingData:
    test_parquet_files = [
        os.path.join(TEST_DATA_DIR, "quote_tick_eurusd_2019_sim_rust.parquet"),
        os.path.join(TEST_DATA_DIR, "quote_tick_usdjpy_2019_sim_rust.parquet"),
        os.path.join(TEST_DATA_DIR, "bars_eurusd_2019_sim.parquet"),
    ]

    test_instruments = [
        TestInstrumentProvider.default_fx_ccy("EUR/USD", venue=Venue("SIM")),
        TestInstrumentProvider.default_fx_ccy("USD/JPY", venue=Venue("SIM")),
        TestInstrumentProvider.default_fx_ccy("EUR/USD", venue=Venue("SIM")),
    ]
    test_instrument_ids = [x.id for x in test_instruments]


class TestBuffer(TestBatchingData):
    @pytest.mark.parametrize(
        ("trim_timestamp", "expected"),
        [
            [1546383600588999936, 1546383600588999936],  # 4, 4
            [1546383600588999936 + 1, 1546383600588999936],  # 4, 4
            [1546383600588999936 - 1, 1546383600487000064],  # 4, 3
        ],
    )
    def test_removed_chunk_has_correct_last_timestamp(
        self,
        trim_timestamp: int,
        expected: int,
    ) -> None:
        # Arrange
        buffer = _StreamingBuffer(
            generate_batches_rust(
                files=[self.test_parquet_files[0]],
                cls=QuoteTick,
                batch_size=10,
            ),
        )

        # Act
        buffer.add_data()
        removed = buffer.remove_front(trim_timestamp)  # timestamp exists

        # Assert
        assert removed[-1].ts_init == expected

    @pytest.mark.parametrize(
        ("trim_timestamp", "expected"),
        [
            [1546383600588999936, 1546383600691000064],  # 4, 5
            [1546383600588999936 + 1, 1546383600691000064],  # 4, 5
            [1546383600588999936 - 1, 1546383600588999936],  # 4, 4
        ],
    )
    def test_streaming_buffer_remove_front_has_correct_next_timestamp(
        self,
        trim_timestamp: int,
        expected: int,
    ) -> None:
        # Arrange
        buffer = _StreamingBuffer(
            generate_batches_rust(
                files=[self.test_parquet_files[0]],
                cls=QuoteTick,
                batch_size=10,
            ),
        )

        # Act
        buffer.add_data()
        buffer.remove_front(trim_timestamp)  # timestamp exists

        # Assert
        next_timestamp = buffer._data[0].ts_init
        assert next_timestamp == expected


class TestBufferIterator(TestBatchingData):
    def test_iterate_returns_expected_timestamps_single(self) -> None:
        # Arrange
        batches = generate_batches_rust(
            files=[self.test_parquet_files[0]],
            cls=QuoteTick,
            batch_size=1000,
        )

        buffer = _StreamingBuffer(batches=batches)

        iterator = _BufferIterator(buffers=[buffer])

        expected = list(pd.read_parquet(self.test_parquet_files[0]).ts_event)

        # Act
        timestamps = []
        for batch in iterator:
            timestamps.extend([x.ts_init for x in batch])

        # Assert
        assert len(timestamps) == len(expected)
        assert timestamps == expected

    def test_iterate_returns_expected_timestamps(self) -> None:
        # Arrange
        expected = sorted(
            list(pd.read_parquet(self.test_parquet_files[0]).ts_event)
            + list(pd.read_parquet(self.test_parquet_files[1]).ts_event),
        )

        buffers = [
            _StreamingBuffer(
                generate_batches_rust(
                    files=[self.test_parquet_files[0]],
                    cls=QuoteTick,
                    batch_size=1000,
                ),
            ),
            _StreamingBuffer(
                generate_batches_rust(
                    files=[self.test_parquet_files[1]],
                    cls=QuoteTick,
                    batch_size=1000,
                ),
            ),
        ]

        iterator = _BufferIterator(buffers=buffers)

        # Act
        timestamps = []
        for batch in iterator:
            timestamps.extend([x.ts_init for x in batch])

        # Assert
        assert len(timestamps) == len(expected)
        assert timestamps == expected

    def test_iterate_returns_expected_timestamps_with_start_end_range_rust(self) -> None:
        # Arrange
        start_timestamps = (1546383605776999936, 1546389021944999936)
        end_timestamps = (1546390125908000000, 1546394394948999936)
        buffers = [
            _StreamingBuffer(
                generate_batches_rust(
                    files=[self.test_parquet_files[0]],
                    cls=QuoteTick,
                    batch_size=1000,
                    start_nanos=start_timestamps[0],
                    end_nanos=end_timestamps[0],
                ),
            ),
            _StreamingBuffer(
                generate_batches_rust(
                    files=[self.test_parquet_files[1]],
                    cls=QuoteTick,
                    batch_size=1000,
                    start_nanos=start_timestamps[1],
                    end_nanos=end_timestamps[1],
                ),
            ),
        ]

        buffer_iterator = _BufferIterator(buffers=buffers)

        # Act
        objs = []
        for batch in buffer_iterator:
            objs.extend(batch)

        # Assert
        instrument_1_timestamps = [
            x.ts_init for x in objs if x.instrument_id == self.test_instrument_ids[0]
        ]
        instrument_2_timestamps = [
            x.ts_init for x in objs if x.instrument_id == self.test_instrument_ids[1]
        ]
        assert instrument_1_timestamps[0] == start_timestamps[0]
        assert instrument_1_timestamps[-1] == end_timestamps[0]

        assert instrument_2_timestamps[0] == start_timestamps[1]
        assert instrument_2_timestamps[-1] == end_timestamps[1]

        timestamps = [x.ts_init for x in objs]
        assert timestamps == sorted(timestamps)

    def test_iterate_returns_expected_timestamps_with_start_end_range_and_bars(self) -> None:
        # Arrange
        start_timestamps = (1546383605776999936, 1546389021944999936, 1559224800000000000)
        end_timestamps = (1546390125908000000, 1546394394948999936, 1577710800000000000)

        buffers = [
            _StreamingBuffer(
                generate_batches_rust(
                    files=[self.test_parquet_files[0]],
                    cls=QuoteTick,
                    batch_size=1000,
                    start_nanos=start_timestamps[0],
                    end_nanos=end_timestamps[0],
                ),
            ),
            _StreamingBuffer(
                generate_batches_rust(
                    files=[self.test_parquet_files[1]],
                    cls=QuoteTick,
                    batch_size=1000,
                    start_nanos=start_timestamps[1],
                    end_nanos=end_timestamps[1],
                ),
            ),
            _StreamingBuffer(
                generate_batches_rust(
                    files=[self.test_parquet_files[2]],
                    cls=Bar,
                    batch_size=1000,
                    start_nanos=start_timestamps[2],
                    end_nanos=end_timestamps[2],
                ),
            ),
        ]

        # Act
        results = []
        buffer_iterator = _BufferIterator(buffers=buffers)

        for batch in buffer_iterator:
            results.extend(batch)

        # Assert
        bars = [x for x in results if isinstance(x, Bar)]
        quote_ticks = [x for x in results if isinstance(x, QuoteTick)]

        instrument_1_timestamps = [
            x.ts_init for x in quote_ticks if x.instrument_id == self.test_instrument_ids[0]
        ]
        instrument_2_timestamps = [
            x.ts_init for x in quote_ticks if x.instrument_id == self.test_instrument_ids[1]
        ]
        instrument_3_timestamps = [
            x.ts_init for x in bars if x.bar_type.instrument_id == self.test_instrument_ids[2]
        ]

        assert instrument_1_timestamps[0] == start_timestamps[0]
        assert instrument_1_timestamps[-1] == end_timestamps[0]

        assert instrument_2_timestamps[0] == start_timestamps[1]
        assert instrument_2_timestamps[-1] == end_timestamps[1]

        assert instrument_3_timestamps[0] == start_timestamps[2]
        assert instrument_3_timestamps[-1] == end_timestamps[2]

        timestamps = [x.ts_init for x in results]
        assert timestamps == sorted(timestamps)


class TestPersistenceBatching:
    def setup(self) -> None:
        self.catalog = data_catalog_setup(protocol="memory")
        self.fs: fsspec.AbstractFileSystem = self.catalog.fs

    def teardown(self) -> None:
        # Cleanup
        path = self.catalog.path
        fs = self.catalog.fs
        if fs.exists(path):
            fs.rm(path, recursive=True)

    @pytest.mark.skip("config_to_buffer no longer has get_files")
    def test_batch_files_single(self, betfair_catalog: ParquetDataCatalog) -> None:
        # Arrange
        self.catalog = betfair_catalog

        instrument_ids = [ins.id for ins in self.catalog.instruments()]

        shared_kw = {
            "catalog_path": str(self.catalog.path),
            "catalog_fs_protocol": self.catalog.fs.protocol,
            "data_cls": OrderBookDelta,
        }

        engine = StreamingEngine(
            data_configs=[
                BacktestDataConfig(**shared_kw, instrument_id=instrument_ids[0]),
                BacktestDataConfig(**shared_kw, instrument_id=instrument_ids[1]),
            ],
            target_batch_size_bytes=parse_bytes("10kib"),
        )

        # Act
        timestamp_chunks = []
        for batch in engine:
            timestamp_chunks.append([b.ts_init for b in batch])

        # Assert
        latest_timestamp = 0
        for timestamps in timestamp_chunks:
            assert max(timestamps) > latest_timestamp
            latest_timestamp = max(timestamps)
            assert timestamps == sorted(timestamps)

    @pytest.mark.skip("config_to_buffer no longer has get_files")
    def test_batch_generic_data(self, betfair_catalog):
        # Arrange
        self.catalog = betfair_catalog
        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="memory",
            data_cls=NewsEventData,
            client_id="NewsClient",
        )

        streaming = BetfairTestStubs.streaming_config(
            catalog_path=self.catalog.path,
        )
        engine = BacktestEngineConfig(streaming=streaming)
        run_config = BacktestRunConfig(
            engine=engine,
            data=[data_config],
            venues=[BetfairTestStubs.betfair_venue_config()],
            batch_size_bytes=parse_bytes("1mib"),
        )

        # Act
        node = BacktestNode(configs=[run_config])
        node.run()

        # Assert
        assert node
