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
from unittest.mock import patch

import pytest

from nautilus_trader.persistence.backtest.scanner import scan
from tests.test_kit import PACKAGE_ROOT


TEST_DATA_DIR = str(pathlib.Path(PACKAGE_ROOT).joinpath("data"))

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="test path broken on windows")


@pytest.mark.parametrize(
    "glob, num_files",
    [
        ("**.json", 3),
        ("**.txt", 1),
        ("**.parquet", 2),
        ("**.csv", 11),
    ],
)
def test_scan_paths(glob, num_files):
    files = scan(path=TEST_DATA_DIR, glob_pattern=glob)
    assert len(files) == num_files


def test_scan_chunks():
    # Total size 17338
    files = scan(path=TEST_DATA_DIR, glob_pattern="1.166564490.bz2", chunk_size=50000)
    raw = list(files[0].iter_raw())
    assert len(raw) == 5


def test_scan_file_filter():
    files = scan(path=TEST_DATA_DIR, glob_pattern="*.csv")
    assert len(files) == 11

    files = scan(path=TEST_DATA_DIR, glob_pattern="*jpy*.csv")
    assert len(files) == 3


@patch("nautilus_trader.persistence.backtest.scanner.load_processed_raw_files")
def test_scan_processed(mock_load_processed_raw_files):
    mock_load_processed_raw_files.return_value = [
        TEST_DATA_DIR + "/truefx-audusd-ticks.csv",
        TEST_DATA_DIR + "/news_events.csv",
        TEST_DATA_DIR + "/tardis_trades.csv",
    ]
    files = scan(path=TEST_DATA_DIR, glob_pattern="*.csv")
    assert len(files) == 8
