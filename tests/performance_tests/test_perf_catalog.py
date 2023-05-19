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
import shutil
import tempfile

import pytest

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.core.nautilus_pyo3.persistence import DataBackendSession
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetType
from nautilus_trader.persistence.wranglers import list_from_capsule
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from nautilus_trader.test_kit.performance import PerformanceHarness
from tests.unit_tests.persistence.test_catalog import TestPersistenceCatalogFile


# TODO: skip in CI


@pytest.mark.skip(reason="update tests for new API")
class TestCatalogPerformance(PerformanceHarness):
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

    @staticmethod
    def test_load_single_stream_catalog_v2(benchmark):
        def setup():
            file_path = os.path.join(PACKAGE_ROOT, "bench_data/quotes_0005.parquet")
            session = DataBackendSession()
            session.add_file("quote_ticks", file_path, ParquetType.QuoteTick)
            return (session.to_query_result(),), {}

        def run(result):
            count = 0
            for chunk in result:
                count += len(list_from_capsule(chunk))

            assert count == 9689614

        benchmark.pedantic(run, setup=setup, rounds=1, iterations=1, warmup_rounds=1)

    @staticmethod
    def test_load_multi_stream_catalog_v2(benchmark):
        def setup():
            dir_path = os.path.join(PACKAGE_ROOT, "bench_data/multi_stream_data/")

            session = DataBackendSession()

            for dirpath, _, filenames in os.walk(dir_path):
                for filename in filenames:
                    if filename.endswith("parquet"):
                        file_stem = os.path.splitext(filename)[0]
                        if "quotes" in filename:
                            full_path = os.path.join(dirpath, filename)
                            session.add_file(file_stem, full_path, ParquetType.QuoteTick)
                        elif "trades" in filename:
                            full_path = os.path.join(dirpath, filename)
                            session.add_file(file_stem, full_path, ParquetType.TradeTick)

            return (session.to_query_result(),), {}

        def run(result):
            count = 0
            for chunk in result:
                ticks = list_from_capsule(chunk)
                count += len(ticks)

            # check total count is correct
            assert count == 72536038

        benchmark.pedantic(run, setup=setup, rounds=1, iterations=1, warmup_rounds=1)
