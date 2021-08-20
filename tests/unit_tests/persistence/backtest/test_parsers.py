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

import pathlib
import sys
from typing import Callable

import pytest

from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import scan_files
from nautilus_trader.persistence.external.parsers import ByteReader
from nautilus_trader.persistence.external.parsers import CSVReader
from nautilus_trader.persistence.external.parsers import ParquetReader
from nautilus_trader.persistence.external.parsers import TextReader
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks import MockReader
from tests.test_kit.mocks import data_catalog_setup


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")


class TestPersistenceParsers:
    def setup(self):
        data_catalog_setup()
        self.catalog = DataCatalog.from_env()
        self.reader = MockReader()

    @pytest.mark.parametrize(
        "glob, parser, expected",
        [
            ("binance*.txt", "parse_text", {"binance-btcusdt-instrument.txt": 1}),
        ],
        indirect=["parser"],
    )
    def test_text_parser(self, glob, parser, expected):
        reader = TextReader(line_parser=parser)
        files = scan_files(glob_path=f"{TEST_DATA_DIR}/{glob}")
        results = {}
        for f in files:
            f.reader = reader
            data = []
            for chunk in f.iter_parsed():
                data.extend(chunk)
            results[f.name] = len(data)
        assert results == expected

    @pytest.mark.parametrize(
        "glob, parser, expected",
        [
            (
                "truefx*.csv",
                "parse_csv_quotes",
                {"truefx-audusd-ticks.csv": 100000, "truefx-usdjpy-ticks.csv": 1000},
            ),
            # TODO (bm)
            # ("fxcm*.csv", "parse_csv_quotes", {}),
            # ("binance*.csv", "parse_csv_quotes", {}),
        ],
        indirect=["parser"],
    )
    def test_csv_quoter_parser(self, glob, parser, expected):
        files = scan_files(glob_path=f"{TEST_DATA_DIR}/{glob}")

        results = {}
        for f in files:
            f.reader = CSVReader(block_parser=parser, as_dataframe=True)
            data = []
            for chunk in f.iter_parsed():
                data.extend(chunk)
            results[f.name] = len(data)
        assert results == expected

    @pytest.mark.parametrize(
        "glob, parser, expected",
        [
            (
                "betfair/*.bz2",
                "parse_betfair",
                {"1.166811431.bz2": 17848, "1.180305278.bz2": 15736},
            ),
        ],
        indirect=["parser"],
    )
    def test_test_parser(self, glob, parser: Callable, expected):
        provider = BetfairInstrumentProvider.from_instruments([])
        reader = BetfairTestStubs.betfair_reader()(provider)  # type: TextReader
        files = scan_files(glob_path=f"{TEST_DATA_DIR}/{glob}")
        results = {}
        for f in files:
            f.reader = reader
            data = []
            for chunk in f.iter_parsed():
                data.extend(chunk)
            results[f.name] = len(data)
        assert results == expected

    def test_byte_json_parser(
        self,
    ):
        files = scan_files(path=TEST_DATA_DIR, glob_pattern="*.json")
        results = {}
        for f in files:
            f.parser = ByteReader()
            data = list(f.iter_parsed())
            results[f.name] = len(data)
        expected = {}
        assert results == expected

    def test_parquet_parser(
        self,
    ):
        files = scan_files(glob_path=f"{TEST_DATA_DIR}/*.parquet")
        results = {}
        for f in files:
            f.parser = ParquetReader()
            data = list(f.iter_parsed())
            results[f.name] = len(data)
        expected = {}
        assert results == expected
