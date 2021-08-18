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

from concurrent.futures import ThreadPoolExecutor
from concurrent.futures import as_completed
from typing import Callable, List

import fsspec
from tqdm import tqdm

from nautilus_trader.persistence.backtest.metadata import load_processed_raw_files
from nautilus_trader.persistence.backtest.parsers import RawFile
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.serialization.arrow.util import identity


def _resolve_path(fs, path: str, glob_pattern):
    if fs.isfile(path):
        return [path]
    # We have a directory
    if not path.endswith("/"):
        path += "/"
    if fs.isdir(path):
        files = fs.glob(f"{path}{glob_pattern}")
        assert files, f"Found no files with path={str(path)}, glob={glob_pattern}"
        return [f for f in files if fs.isfile(f)]
    else:
        raise ValueError("path argument must be str and a valid directory or file")


def _scan(fs: fsspec.AbstractFileSystem, path: str, chunk_size: int, **kwargs):
    return RawFile(fs=fs, path=path, chunk_size=chunk_size, **kwargs)


def _scan_threaded(
    fs: fsspec.AbstractFileSystem,
    paths: List[str],
    progress=True,
    compression="infer",
    chunk_size: int = -1,
    file_filter: Callable = None,
    executor=None,
) -> List[RawFile]:
    """
    Scan `fs` filesystem `paths`, and return a list of `ChunkedFiles`
    """
    executor = executor or ThreadPoolExecutor()
    file_filter = file_filter or identity
    # existing_files = _load_processed_raw_files(
    #     fs=fs,
    # )

    futures = []
    for path in paths:
        if file_filter(path):
            futures.append(
                executor.submit(
                    _scan, fs=fs, path=path, chunk_size=chunk_size, compression=compression
                )
            )

    ac = as_completed(futures)
    if progress:
        ac = tqdm(ac, total=len(futures))

    files = []
    for f in ac:
        files.append(f.result())
    return sorted(files, key=lambda x: x.path)


def scan(
    path: str,
    fs_protocol="file",
    glob_pattern="**",
    progress=True,
    chunk_size=-1,
    compression: str = "infer",
    file_filter: Callable = None,
    executor=None,
    skip_already_processed=True,
) -> List[RawFile]:
    """
    Scan `path` using `glob_pattern` and generate a list files to be loaded into the data catalog.

    Parameters
    ----------
    path : str
        The resolvable path; a file, folder, or a remote location via fsspec.
    fs_protocol : str
        The fsspec protocol; allows remote access - defaults to `file`.
    glob_pattern : str
        The glob pattern to search for files.
    progress : bool
        If progress should be shown when scanning individual files.
    chunk_size : int
        The chunk size (in bytes) for processing data, -1 for no limit (will chunk per file).
    compression : str
        Compression for files, defaults to 'infer' by file extension.
    file_filter: callable
        Optional filter to apply to file list (if glob_pattern is not enough)
    executor: concurrent.futures.Executor
        Optional: pass an executor instance
    skip_already_processed: bool
        Skip already processed files according to `load_processed_raw_files`
    """
    fs = fsspec.filesystem(fs_protocol)
    paths = _resolve_path(fs=fs, path=path, glob_pattern=glob_pattern)
    if skip_already_processed:
        catalog = DataCatalog.from_env()
        existing = load_processed_raw_files(fs=catalog.fs)
        paths = [p for p in paths if str(p) not in existing]
    return _scan_threaded(
        fs=fs,
        paths=paths,
        chunk_size=chunk_size,
        progress=progress,
        file_filter=file_filter,
        executor=executor,
        compression=compression,
    )
