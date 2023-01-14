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

import shutil
import tempfile

import pytest

from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.performance import PerformanceHarness
from tests.unit_tests.persistence.test_catalog import TestPersistenceCatalogFile


@pytest.mark.skip(reason="update tests for new API")
class TestBacktestEnginePerformance(PerformanceHarness):
    @staticmethod
    def test_load_quote_ticks_python(benchmark):

        tempdir = tempfile.mkdtemp()

        def setup():

            # Arrange
            cls = TestPersistenceCatalogFile()

            cls.catalog = data_catalog_setup(protocol="file", path=tempdir)

            cls._load_quote_ticks_into_catalog()

            # Act
            return (cls.catalog,), {}

        def run(catalog):
            quotes = catalog.quote_ticks(as_nautilus=True)
            assert len(quotes) == 9500

        benchmark.pedantic(run, setup=setup, rounds=1, iterations=1, warmup_rounds=1)
        shutil.rmtree(tempdir)

    @staticmethod
    def test_load_quote_ticks_rust(benchmark):

        tempdir = tempfile.mkdtemp()

        def setup():
            # Arrange
            cls = TestPersistenceCatalogFile()

            cls.catalog = data_catalog_setup(protocol="file", path=tempdir)

            cls._load_quote_ticks_into_catalog(use_rust=True)

            # Act
            return (cls.catalog,), {}

        def run(catalog):
            quotes = catalog.quote_ticks(as_nautilus=True, use_rust=True)
            assert len(quotes) == 9500

        benchmark.pedantic(run, setup=setup, rounds=1, iterations=1, warmup_rounds=1)
        shutil.rmtree(tempdir)
