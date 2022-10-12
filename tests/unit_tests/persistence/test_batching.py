# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import heapq
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
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.persistence.batching import batch_files
from nautilus_trader.persistence.batching import generate_batches
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.catalog.parquet import resolve_path
from nautilus_trader.persistence.catalog.rust.reader import ParquetFileReader
from nautilus_trader.persistence.catalog.rust.writer import ParquetWriter
from nautilus_trader.persistence.external.core import process_files
from nautilus_trader.persistence.external.core import write_objects
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.persistence.funcs import parse_bytes
from nautilus_trader.serialization.arrow.util import clean_key
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks.data import NewsEventData
from tests.test_kit.mocks.data import data_catalog_setup
from tests.test_kit.stubs.persistence import TestPersistenceStubs


TEST_DATA_DIR = PACKAGE_ROOT + "/data"


class TestPersistenceBatchingRust:
    def setup(self):
        data_catalog_setup(protocol="file")
        self.catalog = ParquetDataCatalog.from_env()
        self.fs: fsspec.AbstractFileSystem = self.catalog.fs

        self.test_data = {
            "EUR/USD.SIM": os.path.join(PACKAGE_ROOT, "data/quote_tick_eurusd_2019.parquet"),
            "USD/JPY.SIM": os.path.join(PACKAGE_ROOT, "data/quote_tick_usdjpy_2019.parquet"),
        }

        self._load_quote_ticks_into_catalog_rust()

    def _load_quote_ticks_into_catalog_rust(self):

        for instrument_id, parquet_data_path in self.test_data.items():
            assert os.path.exists(parquet_data_path)

            reader = ParquetFileReader(QuoteTick, parquet_data_path)
            quotes = list(itertools.chain(*list(reader)))

            # Use rust writer
            metadata = {
                "instrument_id": instrument_id,
                "price_precision": "5",
                "size_precision": "0",
            }

            writer = ParquetWriter(QuoteTick, metadata)
            writer.write(quotes)
            data: bytes = writer.flush()

            fn = (
                self.catalog.path
                / f"data/quote_tick.parquet/instrument_id={clean_key(instrument_id)}/0-0-0.parquet"
            )

            fn.parent.mkdir(parents=True, exist_ok=True)
            fn.write_bytes(data)

            instrument_id = InstrumentId.from_str(instrument_id)
            instrument = TestInstrumentProvider.default_fx_ccy(
                str(instrument_id.symbol), venue=instrument_id.venue
            )
            write_objects(self.catalog, [instrument])

    def test_generate_batches_rust(self):
        # Arrange
        config = BacktestDataConfig(
            catalog_path=str(self.catalog.path),
            catalog_fs_protocol=self.catalog.fs.protocol,
            data_cls=QuoteTick,
            instrument_id="EUR/USD.SIM",
        )
        batch_gen = generate_batches(self.catalog, config, 300, use_rust=True)

        parquet_data_path = os.path.join(PACKAGE_ROOT, "data/quote_tick_eurusd_2019.parquet")
        assert os.path.exists(parquet_data_path)
        reader = ParquetFileReader(QuoteTick, parquet_data_path)
        expected = list(itertools.chain(*list(reader)))

        # Act
        results = []
        batch = True
        while batch:
            batch = next(batch_gen, [])
            results.extend(batch)

        # Assert
        assert len(results) == len(expected)
        assert pd.Series([x.ts_init for x in results]).equals(
            pd.Series([x.ts_init for x in expected])
        )

    def test_batch_files_single_config_rust(self):
        # Arrange
        instrument_ids = self.catalog.instruments()["id"].unique().tolist()

        base = BacktestDataConfig(
            catalog_path=str(self.catalog.path),
            catalog_fs_protocol=self.catalog.fs.protocol,
            data_cls=QuoteTick,
        )

        parquet_data_path = os.path.join(PACKAGE_ROOT, "data/quote_tick_eurusd_2019.parquet")
        assert os.path.exists(parquet_data_path)
        reader = ParquetFileReader(QuoteTick, parquet_data_path)
        expected = list(itertools.chain(*list(reader)))

        iter_batches = batch_files(
            catalog=self.catalog,
            data_configs=[
                base.replace(instrument_id=instrument_ids[0]),
            ],
            target_batch_size_bytes=parse_bytes("10kib"),
            read_num_rows=300,
            use_rust=True,
        )

        results = []
        for batch in iter_batches:
            results.extend(batch)

        assert len(results) == 10_000
        assert pd.Series([x.ts_init for x in results]).equals(
            pd.Series([x.ts_init for x in expected])
        )

    def test_batch_files_multiple_configs_timestamp_order_rust(self):
        # Arrange
        base = BacktestDataConfig(
            catalog_path=str(self.catalog.path),
            catalog_fs_protocol=self.catalog.fs.protocol,
            data_cls=QuoteTick,
        )

        instrument_ids = self.catalog.instruments()["id"].unique().tolist()

        iter_batches = batch_files(
            catalog=self.catalog,
            data_configs=[
                base.replace(instrument_id=instrument_ids[0]),
                base.replace(instrument_id=instrument_ids[1]),
            ],
            target_batch_size_bytes=parse_bytes("10kib"),
            read_num_rows=300,
            use_rust=True,
        )

        # Act
        timestamp_chunks = []
        for batch in iter_batches:
            timestamp_chunks.append([x.ts_init for x in batch])

        # Assert
        latest_timestamp = 0
        for timestamps in timestamp_chunks:
            assert max(timestamps) > latest_timestamp
            latest_timestamp = max(timestamps)
            assert timestamps == sorted(timestamps)

    def test_batch_files_multiple_configs_data_contents_valid_rust(self):
        # Arrange
        instrument_ids = self.catalog.instruments()["id"].unique().tolist()

        parquet_data_paths = list(self.test_data.values())
        expected = itertools.chain(
            *list(
                [
                    ParquetFileReader(QuoteTick, parquet_data_paths[0]),
                    ParquetFileReader(QuoteTick, parquet_data_paths[1]),
                ]
            )
        )
        expected = list(heapq.merge(*expected, key=lambda x: x.ts_init))

        base = BacktestDataConfig(
            catalog_path=str(self.catalog.path),
            catalog_fs_protocol=self.catalog.fs.protocol,
            data_cls=QuoteTick,
        )

        iter_batches = batch_files(
            catalog=self.catalog,
            data_configs=[
                base.replace(instrument_id=instrument_ids[0]),
                base.replace(instrument_id=instrument_ids[1]),
            ],
            target_batch_size_bytes=parse_bytes("10kib"),
            read_num_rows=300,
            use_rust=True,
        )

        # Act
        results = []
        for batch in iter_batches:
            results.extend(batch)

        # Assert
        expected_instrument_ids = self.test_data.keys()
        result_instrument_ids = {str(x.instrument_id) for x in results}

        for expected_instrument_id in expected_instrument_ids:
            assert expected_instrument_id in result_instrument_ids

        assert len(results) == len(expected)
        assert pd.Series([x.ts_init for x in results]).equals(
            pd.Series([x.ts_init for x in expected])
        )


class TestPersistenceBatching:
    def setup(self):
        data_catalog_setup(protocol="file")
        self.catalog = ParquetDataCatalog.from_env()
        self.fs: fsspec.AbstractFileSystem = self.catalog.fs
        self._load_data_into_catalog()

    def _load_data_into_catalog(self):
        self.instrument_provider = BetfairInstrumentProvider.from_instruments([])
        process_files(
            glob_path=PACKAGE_ROOT + "/data/1.166564490.bz2",
            reader=BetfairTestStubs.betfair_reader(instrument_provider=self.instrument_provider),
            instrument_provider=self.instrument_provider,
            catalog=self.catalog,
        )

    def test_batch_files_single(self):

        # Arrange
        instrument_ids = self.catalog.instruments()["id"].unique().tolist()
        base = BacktestDataConfig(
            catalog_path=str(self.catalog.path),
            catalog_fs_protocol=self.catalog.fs.protocol,
            data_cls=OrderBookData,
        )

        iter_batches = batch_files(
            catalog=self.catalog,
            data_configs=[
                base.replace(instrument_id=instrument_ids[0]),
                base.replace(instrument_id=instrument_ids[1]),
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
            glob_path=f"{PACKAGE_ROOT}/data/news_events.csv",
            reader=CSVReader(block_parser=TestPersistenceStubs.news_event_parser),
            catalog=self.catalog,
        )
        data_config = BacktestDataConfig(
            catalog_path="/.nautilus/catalog/",
            catalog_fs_protocol="memory",
            data_cls=NewsEventData,
            client_id="NewsClient",
        )
        # Add some arbitrary instrument data to appease BacktestEngine
        instrument_data_config = BacktestDataConfig(
            catalog_path="/.nautilus/catalog/",
            catalog_fs_protocol="memory",
            instrument_id=self.catalog.instruments(as_nautilus=True)[0].id.value,
            data_cls=InstrumentStatusUpdate,
        )

        streaming = BetfairTestStubs.streaming_config(
            catalog_path=resolve_path(self.catalog.path, self.fs)
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
