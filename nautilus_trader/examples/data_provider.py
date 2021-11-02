import pathlib
from typing import Optional

import fsspec
from fsspec.implementations.github import GithubFileSystem
from fsspec.implementations.local import LocalFileSystem

from nautilus_trader.backtest.data.loaders import CSVTickDataLoader
from nautilus_trader.backtest.data.loaders import ParquetBarDataLoader
from nautilus_trader.backtest.data.loaders import ParquetTickDataLoader


class TestDataProvider:
    """
    Provides an API to load data from either test/ directory or github repo
    """

    def __init__(
        self,
        branch="develop",
    ):
        self.fs: Optional[fsspec.AbstractFileSystem] = None
        self.root: Optional[str] = None
        self._determine_filesystem()
        self.branch = branch

    def _determine_filesystem(self):
        if test_data_directory():
            self.root = test_data_directory()
            self.fs = fsspec.filesystem("file")
        else:
            print("Couldn't find test data directory, test data will be pulled from github")
            self.root = "tests/test_kit/data"
            self.fs = fsspec.filesystem("github", org="nautechsystems", repo="nautilus_trader")

    def _make_uri(self, path: str):
        if isinstance(self.fs, LocalFileSystem):
            return f"file://{self.root}/{path}"
        elif isinstance(self.fs, GithubFileSystem):
            return f"github://{self.fs.org}:{self.fs.repo}@{self.branch}/{self.root}/{path}"

    def read_csv_ticks(self, path: str):
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return CSVTickDataLoader.load(file_path=f)

    def read_parquet_ticks(self, path: str):
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return ParquetTickDataLoader.load(file_path=f)

    def read_parquet_bars(self, path: str):
        uri = self._make_uri(path=path)
        with fsspec.open(uri) as f:
            return ParquetBarDataLoader.load(file_path=f)


def test_data_directory() -> Optional[str]:
    """Determine if the test data directory exists (i.e., this is a checkout of the source code)"""
    source_root = pathlib.Path(__file__).parent.parent
    assert source_root.stem == "nautilus_trader"
    test_data_dir = source_root.parent.joinpath("tests", "test_kit", "data")
    if test_data_dir.exists():
        return str(test_data_dir)
    else:
        return None


if __name__ == "__main__":
    t = TestDataProvider()
    t.read_csv_ticks("truefx-usdjpy-ticks.csv")
