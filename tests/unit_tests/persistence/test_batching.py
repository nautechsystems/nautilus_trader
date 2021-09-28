# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import functools
import sys

import fsspec
import pandas as pd
import pytest
from dask.utils import parse_bytes

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.persistence.batching import _get_schema_widths
from nautilus_trader.persistence.batching import calc_streaming_chunks
from nautilus_trader.persistence.batching import calculate_data_size
from nautilus_trader.persistence.batching import make_unix_ns
from nautilus_trader.persistence.batching import search_data_size_timestamp
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import process_files
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks import data_catalog_setup


TEST_DATA_DIR = PACKAGE_ROOT + "/data"


@pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")
class TestPersistenceBatching:
    def setup(self):
        data_catalog_setup()
        self.catalog = DataCatalog.from_env()
        self.fs: fsspec.AbstractFileSystem = self.catalog.fs
        self._loaded_data_into_catalog()

    def _loaded_data_into_catalog(self):
        self.instrument_provider = BetfairInstrumentProvider.from_instruments([])
        process_files(
            glob_path=PACKAGE_ROOT + "/data/1.166564490.bz2",
            reader=BetfairTestStubs.betfair_reader(instrument_provider=self.instrument_provider),
            instrument_provider=self.instrument_provider,
            catalog=self.catalog,
        )

    def test_calculate_data_size(self):
        # Arrange
        instrument_ids = self.catalog.instruments()["id"].tolist()
        func = functools.partial(
            calculate_data_size,
            root_path=self.catalog.path,
            fs=self.catalog.fs,
            instrument_ids=instrument_ids,
            data_types=[TradeTick],
            start_time=1576840503572000000,
        )

        # Act
        results = [
            func(end_time=make_unix_ns("2019-12-20 11:30:00")),
            func(end_time=make_unix_ns("2019-12-20 15:00:00")),
            func(end_time=make_unix_ns("2019-12-20 18:00:00")),
            func(end_time=make_unix_ns("2019-12-20 22:00:00")),
        ]

        # Assert
        expected = [2695, 16981, 30188, 84093]
        assert results == expected

    def test_search_data_size_timestamp(self):
        # Arrange
        instrument_ids = self.catalog.instruments()["id"].tolist()

        # Act
        target_func = search_data_size_timestamp(
            root_path=self.catalog.path,
            fs=self.catalog.fs,
            instrument_ids=instrument_ids,
            data_types=[TradeTick],
            start_time=1576840503572000000,
            target_size=parse_bytes("1mib"),
        )

        # Assert
        result = target_func([1576878597067000000])
        assert int(result) == 964483

    def test_get_schema_widths(self):
        # Arrange
        instrument_id = self.catalog.instruments()["id"][0]
        fn = f"{self.catalog.path}/data/trade_tick.parquet/instrument_id={instrument_id}"

        # TODO - compare widths * rows with df memory usage
        # df = pd.read_parquet(fn, filesystem=self.fs)
        # mem_usage = df.memory_usage(index=False, deep=True)

        # Act
        widths = _get_schema_widths(path=fn, fs=self.fs)

        # Assert
        expected = {
            "aggressor_side": 4.0,
            "match_id": 121.0,
            "price": 64.0,
            "size": 64.60606060606061,
            "ts_event": 8.0,
            "ts_init": 8.0,
        }
        assert widths == expected

    def test_generate_data_batches_perf(self, benchmark):
        # Arrange
        instrument_ids = self.catalog.instruments()["id"].tolist()

        def run():
            return list(
                calc_streaming_chunks(
                    catalog=self.catalog,
                    instrument_ids=instrument_ids,
                    data_types=[TradeTick],
                    start_time=make_unix_ns("2019-12-20"),
                    end_time=make_unix_ns("2019-12-21"),
                    target_size=parse_bytes("15kib"),
                    debug=False,
                )
            )

        result = benchmark.pedantic(target=run, rounds=5, iterations=5)
        assert result

    def test_calc_streaming_chunks_results(self):
        # Arrange
        instrument_ids = self.catalog.instruments()["id"].tolist()

        # Act
        it = calc_streaming_chunks(
            catalog=self.catalog,
            instrument_ids=instrument_ids,
            data_types=[TradeTick],
            start_time="2019-12-20",
            end_time="2019-12-21",
            target_size=parse_bytes("15kib"),
            debug=False,
        )

        # Assert
        result = [(pd.Timestamp(s).isoformat(), pd.Timestamp(e).isoformat()) for s, e in it]
        expected = [
            ("2019-12-20T00:00:00", "2019-12-20T14:23:12.078341376"),
            ("2019-12-20T14:23:12.078341376", "2019-12-20T17:24:00.368340736"),
            ("2019-12-20T17:24:00.368340736", "2019-12-20T19:45:12.032017664"),
            ("2019-12-20T19:45:12.032017664", "2019-12-20T20:45:21.035707136"),
            ("2019-12-20T20:45:21.035707136", "2019-12-20T23:59:59.999870464"),
            ("2019-12-20T23:59:59.999870464", "2019-12-21T00:00:00"),
        ]
        assert result == expected
