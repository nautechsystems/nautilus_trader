from unittest.mock import patch

import pytest
from fsspec.implementations.ftp import FTPFileSystem
from fsspec.implementations.local import LocalFileSystem

from nautilus_trader.persistence.external.metadata import _glob_path_to_fs


CASES = [
    ("/home/test/file.csv", LocalFileSystem, {"protocol": "file"}),
    (
        "ftp://test@0.0.0.0/home/test/file.csv",  # noqa: S104
        FTPFileSystem,
        {"host": "0.0.0.0", "protocol": "ftp", "username": "test"},  # noqa: S104
    ),
]


@patch("nautilus_trader.persistence.external.metadata.fsspec.filesystem")
@pytest.mark.parametrize("glob, kw", [(path, kw) for path, _, kw in CASES])
def test_glob_path_to_fs_inferred(mock, glob, kw):
    _glob_path_to_fs(glob)
    mock.assert_called_with(**kw)


@pytest.mark.parametrize("glob, cls", [(path, cls) for path, cls, _ in CASES])
def test_glob_path_to_fs(glob, cls):
    fs = _glob_path_to_fs(glob)
    assert isinstance(fs, cls)
