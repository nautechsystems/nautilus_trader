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

import itertools
import os

import fsspec
import pandas as pd

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetReader
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetReaderType
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.persistence.batching import batch_files
from nautilus_trader.persistence.batching import generate_batches
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.persistence.funcs import parse_bytes
from nautilus_trader.test_kit.mocks.data import NewsEventData
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.stubs.persistence import TestPersistenceStubs
from tests import TEST_DATA_DIR
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


class TestPersistenceBatching:
    def setup(self):
        self.catalog = data_catalog_setup(protocol="memory")
        self.fs: fsspec.AbstractFileSystem = self.catalog.fs
        self._load_data_into_catalog()

    def teardown(self):
        # Cleanup
        path = self.catalog.path
        fs = self.catalog.fs
        if fs.exists(path):
            fs.rm(path, recursive=True)

    def _load_data_into_catalog(self):
        self.instrument_provider = BetfairInstrumentProvider.from_instruments([])
        process_files(
            glob_path=TEST_DATA_DIR + "/1.166564490.bz2",
            reader=BetfairTestStubs.betfair_reader(instrument_provider=self.instrument_provider),
            instrument_provider=self.instrument_provider,
            catalog=self.catalog,
        )

    def test_batch_files_single(self):
        # Arrange
        instrument_ids = self.catalog.instruments()["id"].unique().tolist()
        shared_kw = dict(
            catalog_path=str(self.catalog.path),
            catalog_fs_protocol=self.catalog.fs.protocol,
            data_cls=OrderBookData,
        )
        iter_batches = batch_files(
            catalog=self.catalog,
            data_configs=[
                BacktestDataConfig(**shared_kw, instrument_id=instrument_ids[0]),
                BacktestDataConfig(**shared_kw, instrument_id=instrument_ids[1]),
            ],
            target_batch_size_bytes=parse_bytes("10kib"),
            read_num_rows=300,
        )

        # Act
        timestamp_chunks = []
        for batch in iter_batches:
            timestamp_chunks.append([b.ts_init for b in batch])

        # Assert
        latest_timestamp = 0
        for timestamps in timestamp_chunks:
            assert max(timestamps) > latest_timestamp
            latest_timestamp = max(timestamps)
            assert timestamps == sorted(timestamps)

    def test_batch_generic_data(self):
        # Arrange
        TestPersistenceStubs.setup_news_event_persistence()
        process_files(
            glob_path=f"{TEST_DATA_DIR}/news_events.csv",
            reader=CSVReader(block_parser=TestPersistenceStubs.news_event_parser),
            catalog=self.catalog,
        )
        data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="memory",
            data_cls=NewsEventData,
            client_id="NewsClient",
        )
        # Add some arbitrary instrument data to appease BacktestEngine
        instrument_data_config = BacktestDataConfig(
            catalog_path=self.catalog.path,
            catalog_fs_protocol="memory",
            instrument_id=self.catalog.instruments(as_nautilus=True)[0].id.value,
            data_cls=InstrumentStatusUpdate,
        )
        streaming = BetfairTestStubs.streaming_config(
            catalog_path=self.catalog.path,
        )
        engine = BacktestEngineConfig(streaming=streaming)
        run_config = BacktestRunConfig(
            engine=engine,
            data=[data_config, instrument_data_config],
            venues=[BetfairTestStubs.betfair_venue_config()],
            batch_size_bytes=parse_bytes("1mib"),
        )

        # Act
        node = BacktestNode(configs=[run_config])
        node.run()

        # Assert
        assert node


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


class TestGenerateBatches(TestBatchingData):
    def test_generate_batches_returns_empty_list_before_start_timestamp_with_end_timestamp(self):

        start_timestamp = 1546389021944999936
        batch_gen = generate_batches(
            files=[self.test_parquet_files[1]],
            cls=QuoteTick,
            fs=fsspec.filesystem("file"),
            n_rows=1000,
            use_rust=True,
            start_time=start_timestamp,
            end_time=1546394394948999936,
        )
        batches = list(batch_gen)
        assert [len(x) for x in batches] == [0, 0, 0, 0, 172, 1000, 1000, 1000, 1000, 887]
        assert batches[4][0].ts_init == start_timestamp

        #################################
        batch_gen = generate_batches(
            files=[self.test_parquet_files[1]],
            cls=QuoteTick,
            fs=fsspec.filesystem("file"),
            n_rows=1000,
            use_rust=True,
            start_time=start_timestamp - 1,
            end_time=1546394394948999936,
        )
        batches = list(batch_gen)
        assert [len(x) for x in batches] == [0, 0, 0, 0, 172, 1000, 1000, 1000, 1000, 887]
        assert batches[4][0].ts_init == start_timestamp

    def test_generate_batches_returns_batches_of_expected_size(self):
        batch_gen = generate_batches(
            files=[self.test_parquet_files[1]],
            cls=QuoteTick,
            fs=fsspec.filesystem("file"),
            n_rows=1000,
            use_rust=True,
        )
        batches = list(batch_gen)
        assert all([len(x) == 1000 for x in batches])

    def test_generate_batches_returns_empty_list_before_start_timestamp(self):

        # Arrange
        parquet_data_path = self.test_parquet_files[0]
        start_timestamp = 1546383601403000064  # index 10 (1st item in batch)
        batch_gen = generate_batches(
            files=[parquet_data_path],
            cls=QuoteTick,
            fs=fsspec.filesystem("file"),
            use_rust=True,
            n_rows=10,
            start_time=start_timestamp,
        )

        # Act
        batch = next(batch_gen, None)

        # Assert
        assert batch == []

        #############################################
        # Arrange
        parquet_data_path = self.test_parquet_files[0]
        start_timestamp = 1546383601862999808  # index 18 (last item in batch)
        batch_gen = generate_batches(
            files=[parquet_data_path],
            cls=QuoteTick,
            fs=fsspec.filesystem("file"),
            use_rust=True,
            n_rows=10,
            start_time=start_timestamp,
        )
        # Act
        batch = next(batch_gen, None)

        # Assert
        assert batch == []

        ###################################################
        # Arrange
        parquet_data_path = self.test_parquet_files[0]
        start_timestamp = 1546383601352000000  # index 9
        batch_gen = generate_batches(
            files=[parquet_data_path],
            cls=QuoteTick,
            fs=fsspec.filesystem("file"),
            use_rust=True,
            n_rows=10,
            start_time=start_timestamp,
        )

        # Act
        batch = next(batch_gen, None)

        # Assert
        assert batch != []

    def test_generate_batches_trims_first_batch_by_start_timestamp(self):
        def create_test_batch_gen(start_timestamp):
            parquet_data_path = self.test_parquet_files[0]
            return generate_batches(
                files=[parquet_data_path],
                cls=QuoteTick,
                fs=fsspec.filesystem("file"),
                use_rust=True,
                n_rows=10,
                start_time=start_timestamp,
            )

        start_timestamp = 1546383605776999936
        batches = list(
            generate_batches(
                files=[self.test_parquet_files[0]],
                cls=QuoteTick,
                fs=fsspec.filesystem("file"),
                use_rust=True,
                n_rows=300,
                start_time=start_timestamp,
            ),
        )

        first_timestamp = batches[0][0].ts_init
        assert first_timestamp == start_timestamp

        ###############################################################
        # Timestamp, index -1, exists
        start_timestamp = 1546383601301000192  # index 8
        batch_gen = create_test_batch_gen(start_timestamp)

        # Act
        batches = list(batch_gen)

        # Assert
        first_timestamp = batches[0][0].ts_init
        assert first_timestamp == start_timestamp

        ###############################################################
        # Timestamp, index 0, exists
        start_timestamp = 1546383600078000128  # index 0
        batch_gen = create_test_batch_gen(start_timestamp)

        # Act
        batches = list(batch_gen)

        # Assert
        first_timestamp = batches[0][0].ts_init
        assert first_timestamp == start_timestamp

        ###############################################################
        # Timestamp, index 0, NOT exists
        start_timestamp = 1546383600078000128  # index 0
        batch_gen = create_test_batch_gen(start_timestamp - 1)

        # Act
        batches = list(batch_gen)

        # Assert
        first_timestamp = batches[0][0].ts_init
        assert first_timestamp == start_timestamp

        ###############################################################
        # Timestamp, index -1, NOT exists
        start_timestamp = 1546383601301000192  # index 8
        batch_gen = create_test_batch_gen(start_timestamp - 1)

        # Act
        batches = list(batch_gen)

        # Assert
        first_timestamp = batches[0][0].ts_init
        assert first_timestamp == start_timestamp
        ###############################################################
        # Arrange

        start_timestamp = 1546383600691000064
        batch_gen = create_test_batch_gen(start_timestamp)

        # Act
        batches = list(batch_gen)

        # Assert
        first_batch = batches[0]
        print(len(first_batch))
        assert len(first_batch) == 5

        first_timestamp = first_batch[0].ts_init
        assert first_timestamp == start_timestamp
        ###############################################################
        # Starts on next timestamp if start_timestamp NOT exists
        # Arrange
        start_timestamp = 1546383600078000128  # index 0
        next_timestamp = 1546383600180000000  # index 1
        batch_gen = create_test_batch_gen(start_timestamp + 1)

        # Act
        batches = list(batch_gen)

        # Assert
        first_timestamp = batches[0][0].ts_init
        assert first_timestamp == next_timestamp

    def test_generate_batches_trims_end_batch_returns_no_empty_batch(self):
        parquet_data_path = self.test_parquet_files[0]

        # Timestamp, index -1, NOT exists
        # Arrange
        end_timestamp = 1546383601914000128  # index 19
        batch_gen = generate_batches(
            files=[parquet_data_path],
            cls=QuoteTick,
            fs=fsspec.filesystem("file"),
            use_rust=True,
            n_rows=10,
            end_time=end_timestamp,
        )

        # Act
        batches = list(batch_gen)

        # Assert
        last_batch = batches[-1]
        assert last_batch != []

    def test_generate_batches_trims_end_batch_by_end_timestamp(self):
        def create_test_batch_gen(end_timestamp):
            parquet_data_path = self.test_parquet_files[0]
            return generate_batches(
                files=[parquet_data_path],
                cls=QuoteTick,
                fs=fsspec.filesystem("file"),
                use_rust=True,
                n_rows=10,
                end_time=end_timestamp,
            )

        ###############################################################
        # Timestamp, index 0
        end_timestamp = 1546383601403000064  # index 10
        batches = list(create_test_batch_gen(end_timestamp))
        last_timestamp = batches[-1][-1].ts_init
        assert last_timestamp == end_timestamp

        batches = list(create_test_batch_gen(end_timestamp + 1))
        last_timestamp = batches[-1][-1].ts_init
        assert last_timestamp == end_timestamp

        ###############################################################
        # Timestamp index -1
        end_timestamp = 1546383601914000128  # index 19

        batches = list(create_test_batch_gen(end_timestamp))
        last_timestamp = batches[-1][-1].ts_init
        assert last_timestamp == end_timestamp

        batches = list(create_test_batch_gen(end_timestamp + 1))
        last_timestamp = batches[-1][-1].ts_init
        assert last_timestamp == end_timestamp

        ###############################################################
        # Ends on prev timestamp

        end_timestamp = 1546383601301000192  # index 8
        prev_timestamp = 1546383601197999872  # index 7
        batches = list(create_test_batch_gen(end_timestamp - 1))
        last_timestamp = batches[-1][-1].ts_init
        assert last_timestamp == prev_timestamp

    def test_generate_batches_returns_valid_data(self):
        # Arrange
        parquet_data_path = self.test_parquet_files[0]
        batch_gen = generate_batches(
            files=[parquet_data_path],
            cls=QuoteTick,
            fs=fsspec.filesystem("file"),
            use_rust=True,
            n_rows=300,
        )
        reader = ParquetReader(
            parquet_data_path,
            1000,
            ParquetType.QuoteTick,
            ParquetReaderType.File,
        )
        mapped_chunk = map(QuoteTick.list_from_capsule, reader)
        expected = list(itertools.chain(*mapped_chunk))

        # Act
        results = []
        for batch in batch_gen:
            results.extend(batch)

        # Assert
        assert len(results) == len(expected)
        assert pd.Series([x.ts_init for x in results]).equals(
            pd.Series([x.ts_init for x in expected]),
        )

    def test_generate_batches_returns_has_inclusive_start_and_end(self):
        # Arrange
        parquet_data_path = self.test_parquet_files[0]

        reader = ParquetReader(
            parquet_data_path,
            1000,
            ParquetType.QuoteTick,
            ParquetReaderType.File,
        )
        mapped_chunk = map(QuoteTick.list_from_capsule, reader)
        expected = list(itertools.chain(*mapped_chunk))

        batch_gen = generate_batches(
            files=[parquet_data_path],
            cls=QuoteTick,
            fs=fsspec.filesystem("file"),
            use_rust=True,
            n_rows=500,
            start_time=expected[0].ts_init,
            end_time=expected[-1].ts_init,
        )

        # Act
        results = []
        for batch in batch_gen:
            results.extend(batch)

        # Assert
        assert len(results) == len(expected)
        assert pd.Series([x.ts_init for x in results]).equals(
            pd.Series([x.ts_init for x in expected]),
        )
