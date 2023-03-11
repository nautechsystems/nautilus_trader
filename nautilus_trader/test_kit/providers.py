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

import pathlib
from typing import Optional

import fsspec
import pandas as pd
from fsspec.implementations.local import LocalFileSystem

from nautilus_trader.persistence.loaders import CSVBarDataLoader
from nautilus_trader.persistence.loaders import CSVTickDataLoader
from nautilus_trader.persistence.loaders import ParquetBarDataLoader
from nautilus_trader.persistence.loaders import ParquetTickDataLoader


class TestDataProvider:
    """
    Provides an API to load data from either the 'test/' directory or GitHub repo.

    Parameters
    ----------
    branch : str
        The NautilusTrader GitHub branch for the path.
    """

    def __init__(self, branch="develop"):
        self.fs: Optional[fsspec.AbstractFileSystem] = None
        self.root: Optional[str] = None
        self._determine_filesystem()
        self.branch = branch

    @staticmethod
    def _test_data_directory() -> Optional[str]:
        # Determine if the test data directory exists (i.e. this is a checkout of the source code).
        source_root = pathlib.Path(__file__).parent.parent
        assert source_root.stem == "nautilus_trader"
        test_data_dir = source_root.parent.joinpath("tests", "test_data")
        if test_data_dir.exists():
            return str(test_data_dir)
        else:
            return None

    def _determine_filesystem(self):
        test_data_dir = TestDataProvider._test_data_directory()
        if test_data_dir:
            self.root = test_data_dir
            self.fs = fsspec.filesystem("file")
        else:
            print("Couldn't find test data directory, test data will be pulled from GitHub")
            self.root = "tests/test_data"
            self.fs = fsspec.filesystem("github", org="nautechsystems", repo="nautilus_trader")

    def _make_uri(self, path: str):
        # Moved here from top level import because GithubFileSystem has extra deps we may not have installed.
        from fsspec.implementations.github import GithubFileSystem

        if isinstance(self.fs, LocalFileSystem):
            return f"file://{self.root}/{path}"
        elif isinstance(self.fs, GithubFileSystem):
            return f"github://{self.fs.org}:{self.fs.repo}@{self.branch}/{self.root}/{path}"

    def read(self, path: str):
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return f.read()

    def read_csv(self, path: str, **kwargs):
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return pd.read_csv(f, **kwargs)

    def read_csv_ticks(self, path: str):
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return CSVTickDataLoader.load(file_path=f)

    def read_csv_bars(self, path: str):
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return CSVBarDataLoader.load(file_path=f)

    def read_parquet_ticks(self, path: str, timestamp_column: str = "timestamp"):
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return ParquetTickDataLoader.load(file_path=f, timestamp_column=timestamp_column)

    def read_parquet_bars(self, path: str):
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return ParquetBarDataLoader.load(file_path=f)
