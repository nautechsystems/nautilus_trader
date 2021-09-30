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

import sys

import fsspec
import pandas as pd
import pytest

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.persistence.batching import batch_files
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

    def test_calc_streaming_chunks_results(self):
        # Arrange
        # Act
        it = batch_files(
            catalog=self.catalog,
            data_configs=[],
            start_time="2019-12-20",
            end_time="2019-12-21",
        )

        # Assert
        result = [(pd.Timestamp(s).isoformat(), pd.Timestamp(e).isoformat()) for s, e in it]
        expected = [
            ("2019-12-20T00:00:00", "2019-12-20T19:40:18.634184448"),
            ("2019-12-20T19:40:18.634184448", "2019-12-20T22:20:48.447877376"),
            ("2019-12-20T22:20:48.447877376", "2019-12-20T23:59:59.999933952"),
            ("2019-12-20T23:59:59.999933952", "2019-12-21T00:00:00"),
        ]
        assert result == expected
