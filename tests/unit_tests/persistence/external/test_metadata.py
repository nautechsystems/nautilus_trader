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

from unittest.mock import patch

import pytest
from fsspec.implementations.ftp import FTPFileSystem
from fsspec.implementations.local import LocalFileSystem

from nautilus_trader.persistence.external.metadata import _glob_path_to_fs


CASES = [
    ("/home/test/file.csv", LocalFileSystem, {"protocol": "file"}),
    (
        "ftp://test@0.0.0.0/home/test/file.csv",
        FTPFileSystem,
        {"host": "0.0.0.0", "protocol": "ftp", "username": "test"},  # noqa: S104
    ),
]


@patch("nautilus_trader.persistence.external.metadata.fsspec.filesystem")
@pytest.mark.parametrize(("glob", "kw"), [(path, kw) for path, _, kw in CASES])
def test_glob_path_to_fs_inferred(mock, glob, kw):
    _glob_path_to_fs(glob)
    mock.assert_called_with(**kw)


@patch("fsspec.implementations.ftp.FTPFileSystem._connect")
@patch("fsspec.implementations.ftp.FTPFileSystem.__del__")
@pytest.mark.parametrize(("glob", "cls"), [(path, cls) for path, cls, _ in CASES])
def test_glob_path_to_fs(_mock1, _mock2, glob, cls):
    fs = _glob_path_to_fs(glob)
    assert isinstance(fs, cls)
