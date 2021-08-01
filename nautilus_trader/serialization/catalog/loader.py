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

import warnings
from itertools import takewhile
from typing import Callable

import fsspec
from tqdm import tqdm

from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.serialization.arrow.util import identity
from nautilus_trader.serialization.catalog.parsers import ByteParser
from nautilus_trader.serialization.catalog.parsers import EOStream
from nautilus_trader.serialization.catalog.parsers import NewFile


class DataLoader:
    """
    Provides general data loading functionality.

    Discover files and stream bytes of data from a local or remote filesystems.
    """

    def __init__(
        self,
        path: str,
        parser: ByteParser,
        fs_protocol="file",
        glob_pattern="**",
        progress=False,
        chunk_size=-1,
        compression: str = "infer",
        instrument_provider=None,
        file_filter: Callable = None,
    ):
        """
        Initialize a new instance of the ``DataLoader`` class.

        Parameters
        ----------
        path : str
            The resolvable path; a file, folder, or a remote location via fsspec.
        parser : ByteParser
            The parser subclass which can convert bytes into Nautilus objects.
        fs_protocol : str
            The fsspec protocol; allows remote access - defaults to `file`.
        glob_pattern : str
            The glob pattern to search for files.
        progress : bool
            If progress should be shown when loading individual files.
        chunk_size : int
            The chunk size (in bytes) for processing data, -1 for no limit (will chunk per file).
        compression : str
            Compression for files, defaults to 'infer' by file extension.
        instrument_provider : InstrumentProvider
            The instrument provider for the loader.
        file_filter: callable
            Optional filter to apply to file list (if glob_pattern is not enough)
        """
        self._path = path
        self.parser = parser
        self.fs_protocol = fs_protocol
        if progress and tqdm is None:
            warnings.warn(
                "tqdm not installed, can't use progress. Install tqdm extra with `pip install nautilus_trader[tqdm]`"
            )
            progress = False
        self.progress = progress
        self.chunk_size = chunk_size
        self.compression = compression
        self.glob_pattern = glob_pattern
        self.fs = fsspec.filesystem(self.fs_protocol)
        self.instrument_provider = instrument_provider
        self.file_filter = file_filter or identity
        self.path = sorted(self._resolve_path(path=self._path))

    def _resolve_path(self, path: str):
        if self.fs.isfile(path):
            return [path]
        # We have a directory
        if not path.endswith("/"):
            path += "/"
        if self.fs.isdir(path):
            files = self.fs.glob(f"{path}{self.glob_pattern}")
            assert files, f"Found no files with path={str(path)}, glob={self.glob_pattern}"
            return [f for f in files if self.fs.isfile(f)]
        else:
            raise ValueError("path argument must be str and a valid directory or file")

    def stream_bytes(self, progress=False):
        path = self.path if not progress else tqdm(self.path)
        for fn in filter(self.file_filter, path):
            with fsspec.open(f"{self.fs_protocol}://{fn}", compression=self.compression) as f:
                yield NewFile(fn)
                data = 1
                while data:
                    try:
                        data = f.read(self.chunk_size)
                    except OSError as e:
                        print(f"ERR file: {fn} ({e}), skipping")
                        break
                    yield data
                    yield None  # this is a chunk
        yield EOStream()

    def run(self, progress=False):
        stream = self.parser.read(
            stream=self.stream_bytes(progress=progress),
            instrument_provider=self.instrument_provider,
        )
        while 1:
            chunk = list(takewhile(lambda x: x is not None, stream))
            if chunk and isinstance(chunk[-1], EOStream):
                break
            if not chunk:
                continue
            # TODO (bm): shithacks - We need a better way to generate instruments?
            if self.instrument_provider is not None:
                # Find any instrument status updates, if we have some, emit instruments first
                instruments = [
                    self.instrument_provider.find(s.instrument_id)
                    for s in chunk
                    if isinstance(s, InstrumentStatusUpdate)
                ]
                chunk = instruments + chunk
            yield chunk
