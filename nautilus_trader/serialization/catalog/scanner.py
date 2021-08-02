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
import orjson
from tqdm import tqdm

from nautilus_trader.serialization.arrow.util import identity
from nautilus_trader.serialization.catalog.parsers import EOStream
from nautilus_trader.serialization.catalog.parsers import NewFile


PROCESSED_FILES_FN = ".processed_raw_files.json"


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


class ChunkedFile(fsspec.core.OpenFile):
    def __init__(self, fs, path, chunk_size: int, **kwargs):
        """
        A subclass of fsspec.OpenFile than can be read in chunks
        """
        super().__init__(fs=fs, path=path, **kwargs)
        self.chunk_size = chunk_size

    def iter_chunks(self):
        with self.open() as f:
            f.seek(0, 2)
            end = f.tell()
            f.seek(0)
            yield NewFile(f.name)
            while f.tell() < end:
                chunk = f.read(self.chunk_size)
                yield chunk
            yield EOStream()


def _scan(fs: fsspec.AbstractFileSystem, path: str, chunk_size: int, **kwargs):
    return ChunkedFile(fs=fs, path=path, chunk_size=chunk_size, **kwargs)


def _scan_threaded(
    fs: fsspec.AbstractFileSystem,
    paths: List[str],
    progress=True,
    compression="infer",
    chunk_size: int = -1,
    file_filter: Callable = None,
    executor=None,
) -> List[ChunkedFile]:
    """
    Scan `fs` filesystem `paths`, and return a list of `ChunkedFiles`
    """
    executor = executor or ThreadPoolExecutor()
    file_filter = file_filter or identity
    existing_files = _load_processed_raw_files(
        fs=fs,
    )

    futures = []
    with executor as client:
        for path in paths:
            if file_filter(path):
                futures.append(
                    client.submit(
                        _scan, fs=fs, path=path, chunk_size=chunk_size, compression=compression
                    )
                )

    ac = as_completed(futures)
    if progress:
        ac = tqdm(ac, total=len(futures))

    files = []
    for f in ac:
        files.append(f.result())
    return files


def scan(
    path: str,
    fs_protocol="file",
    glob_pattern="**",
    progress=True,
    chunk_size=-1,
    compression: str = "infer",
    file_filter: Callable = None,
    executor=None,
) -> List[ChunkedFile]:
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
    """
    fs = fsspec.filesystem(fs_protocol)
    paths = _resolve_path(fs=fs, path=path, glob_pattern=glob_pattern)
    return _scan_threaded(
        fs=fs,
        paths=paths,
        chunk_size=chunk_size,
        progress=progress,
        file_filter=file_filter,
        executor=executor,
        compression=compression,
    )


def _save_processed_raw_files(
    fs: fsspec.AbstractFileSystem, files: List[str], _processed_files_fn: str
):
    # TODO(bm): We should save a hash of the contents alongside the filename to check for changes
    # load existing
    existing = _load_processed_raw_files(fs=fs, _processed_files_fn=_processed_files_fn)
    new = set(files + existing)
    with fs.open(_processed_files_fn, "wb") as f:
        return f.write(orjson.dumps(sorted(new)))


def _load_processed_raw_files(fs: fsspec.AbstractFileSystem, _processed_files_fn: str):
    if fs.exists(_processed_files_fn):
        with fs.open(_processed_files_fn, "rb") as f:
            return orjson.loads(f.read())
    else:
        return []
