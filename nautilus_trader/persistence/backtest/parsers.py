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
import copy
import inspect
import math
import pathlib
from io import BytesIO
from typing import Callable, Generator, List, Optional, Union

import fsspec
import pandas as pd

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.serialization.arrow.util import identity


class Reader:
    def __init__(
        self,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update: Callable = None,
    ):
        """
        Provides parsing of raw byte chunks to Nautilus objects.
        """
        self.instrument_provider = instrument_provider
        self.instrument_provider_update = instrument_provider_update
        self.buffer = b""

    def check_instrument_provider(self, data: Union[bytes, str]):
        if self.instrument_provider_update is not None:
            assert (
                self.instrument_provider is not None
            ), "Passed `instrument_provider_update` but `instrument_provider` was None"
            instruments = set(self.instrument_provider.get_all().values())
            r = self.instrument_provider_update(self.instrument_provider, data)
            # Check the user hasn't accidentally used a generator here also
            if isinstance(r, Generator):
                raise Exception(f"{self.instrument_provider_update} func should not be generator")
            new_instruments = set(self.instrument_provider.get_all().values()).difference(
                instruments
            )
            if new_instruments:
                return list(new_instruments)

    def parse(self, chunk: bytes) -> Generator:
        raise NotImplementedError


class ByteReader(Reader):
    def __init__(
        self,
        byte_parser: Callable,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update: Callable = None,
    ):
        """
        A Reader subclass for reading chunks of raw bytes; `byte_parser` will be passed a chunk of raw bytes.

        Parameters
        ----------
        byte_parser : Callable
            The handler which takes a chunk of bytes and yields Nautilus objects.
        instrument_provider_update : Callable , optional
            An optional hook/callable to update instrument provider before data is passed to `byte_parser`
            (in many cases instruments need to be known ahead of parsing).
        """
        super().__init__(
            instrument_provider_update=instrument_provider_update,
            instrument_provider=instrument_provider,
        )
        assert inspect.isgeneratorfunction(byte_parser)
        self.parser = byte_parser

    def parse(self, chunk: bytes) -> Generator:
        instruments = self.check_instrument_provider(data=chunk)
        if instruments:
            yield from instruments
        yield from self.parser(chunk)


class TextReader(ByteReader):
    def __init__(
        self,
        line_parser: Callable,
        line_preprocessor: Callable = None,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update: Callable = None,
    ):
        """
        A Reader subclass for reading lines of a text-like file; `line_parser` will be passed a single row of bytes.

        Parameters
        ----------
        line_parser : Callable
            The handler which takes byte strings and yields Nautilus objects.
        line_preprocessor : Callable, optional
            The context manager for preprocessing (cleaning log lines) of lines
            before json.loads is called. Nautilus objects are returned to the
            context manager for any post-processing also (for example, setting
            the `ts_init`).
        instrument_provider_update : Callable , optional
            An optional hook/callable to update instrument provider before
            data is passed to `line_parser` (in many cases instruments need to
            be known ahead of parsing).
        """
        super().__init__(
            instrument_provider_update=instrument_provider_update,
            byte_parser=line_parser,
            instrument_provider=instrument_provider,
        )
        self.line_preprocessor = line_preprocessor or identity

    def parse(self, chunk) -> Generator:  # noqa: C901
        self.buffer += chunk
        if b"\n" in chunk:
            process, self.buffer = self.buffer.rsplit(b"\n", maxsplit=1)
        else:
            process, self.buffer = chunk, b""
        if process:
            yield from self.process_chunk(chunk=process)

    def process_chunk(self, chunk):
        for line in map(self.line_preprocessor, chunk.split(b"\n")):
            instruments = self.check_instrument_provider(data=line)
            if instruments:
                yield from instruments
            yield from self.parser(line)


class CSVReader(Reader):
    """
    Provides parsing of CSV formatted bytes strings to Nautilus objects.
    """

    def __init__(
        self,
        chunk_parser: Callable,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update=None,
        chunked=True,
        as_dataframe=False,
    ):
        """
        Initialize a new instance of the ``CSVReader`` class.

        Parameters
        ----------
        chunk_parser : callable
            The handler which takes byte strings and yields Nautilus objects.
        instrument_provider_update
            Optional hook to call before `parser` for the purpose of loading instruments into an InstrumentProvider
        chunked: bool, default=True
            If chunked=False, each CSV line will be passed to `chunk_parser` individually, if chunked=True, the data
            passed will potentially contain many lines (a chunk).
        as_dataframe: bool, default=False
            If as_dataframe=True, the passes chunk will be parsed into a DataFrame before passing to `chunk_parser`

        """
        super().__init__(
            instrument_provider=instrument_provider,
            instrument_provider_update=instrument_provider_update,
        )
        self.chunk_parser = chunk_parser
        self.header: Optional[List[str]] = None
        self.chunked = chunked
        self.as_dataframe = as_dataframe

    def parse(self, chunk: bytes) -> Generator:
        if self.header is None:
            header, chunk = chunk.split(b"\n", maxsplit=1)
            self.header = header.decode().split(",")

        self.buffer += chunk
        process, self.buffer = self.buffer.rsplit(b"\n", maxsplit=1)

        if self.as_dataframe:
            process = pd.read_csv(BytesIO(process), names=self.header)
        if self.instrument_provider_update is not None:
            self.instrument_provider_update(self.instrument_provider, process)
        yield from self.chunk_parser(process)


class ParquetReader(ByteReader):
    """
    Provides parsing of parquet specification bytes to Nautilus objects.
    """

    def __init__(
        self,
        data_type: str,
        parser: Callable = None,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update=None,
    ):
        """
        Initialize a new instance of the ``ParquetParser`` class.

        Parameters
        ----------
        data_type : One of `quote_ticks`, `trade_ticks`
            The wrangler which takes pandas dataframes (from parquet) and yields Nautilus objects.
        instrument_provider_update
            Optional hook to call before `parser` for the purpose of loading instruments into the InstrumentProvider

        """
        data_types = ("quote_ticks", "trade_ticks")
        assert data_type in data_types, f"data_type must be one of {data_types}"
        super().__init__(
            byte_parser=parser,
            instrument_provider_update=instrument_provider_update,
            instrument_provider=instrument_provider,
        )
        self.parser = parser
        self.filename = None
        self.data_type = data_type

    def parse(self, chunk: bytes) -> Generator:
        df = pd.read_parquet(BytesIO(chunk))
        if self.instrument_provider_update is not None:
            self.instrument_provider_update(
                instrument_provider=self.instrument_provider,
                df=df,
                filename=self.filename,
            )
        yield from self.parser(data_type=self.data_type, df=df, filename=self.filename)


class RawFile(fsspec.core.OpenFile):
    def __init__(self, fs: fsspec.AbstractFileSystem, path: str, chunk_size: int = -1, **kwargs):
        """
        A subclass of fsspec.OpenFile than can be read in chunks
        """
        super().__init__(fs=fs, path=path, **kwargs)
        self.name = pathlib.Path(path).name
        self.chunk_size = chunk_size
        self._reader: Optional[Reader] = None

    @property
    def reader(self):
        return self._reader

    @reader.setter
    def reader(self, reader: Reader):
        assert isinstance(reader, Reader)
        self._reader = copy.copy(reader)

    @property
    def num_chunks(self):
        if self.chunk_size == -1:
            return 1
        stat = self.fs.stat(self.path)
        return math.ceil(stat["size"] / self.chunk_size)

    def iter_raw(self):
        with self.open() as f:
            f.seek(0, 2)
            end = f.tell()
            f.seek(0)
            while f.tell() < end:
                chunk = f.read(self.chunk_size)
                yield chunk

    def iter_parsed(self):
        for chunk in self.iter_raw():
            parsed = list(filter(None, self.reader.parse(chunk)))
            yield parsed
