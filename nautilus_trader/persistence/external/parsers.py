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

import inspect
import sys
from io import BytesIO
from typing import Any, Callable, Generator, List, Optional, Union

import pandas as pd

from nautilus_trader.common.providers import InstrumentProvider


PY37 = sys.version_info < (3, 8)


class LinePreprocessor:
    def __call__(self, line: bytes):
        for line_, state in self.pre_process(line):
            obj = yield line_
            yield
            yield self.post_process(obj=obj, state=state)

    @staticmethod
    def pre_process(line):
        return line, {}

    @staticmethod
    def post_process(obj: Any, state: dict):
        return obj


class Reader:
    def __init__(
        self,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update: Callable = None,
    ):
        """
        Provides parsing of raw byte blocks to Nautilus objects.
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

    def parse(self, block: bytes) -> Generator:
        raise NotImplementedError


class ByteReader(Reader):
    def __init__(
        self,
        block_parser: Callable,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update: Callable = None,
    ):
        """
        A Reader subclass for reading blocks of raw bytes; `byte_parser` will be passed a blocks of raw bytes.

        Parameters
        ----------
        block_parser : Callable
            The handler which takes a blocks of bytes and yields Nautilus objects.
        instrument_provider_update : Callable , optional
            An optional hook/callable to update instrument provider before data is passed to `byte_parser`
            (in many cases instruments need to be known ahead of parsing).
        """
        super().__init__(
            instrument_provider_update=instrument_provider_update,
            instrument_provider=instrument_provider,
        )
        if not PY37:
            assert inspect.isgeneratorfunction(block_parser)
        self.parser = block_parser

    def parse(self, block: bytes) -> Generator:
        instruments = self.check_instrument_provider(data=block)
        if instruments:
            yield from instruments
        yield from self.parser(block)


class TextReader(ByteReader):
    def __init__(
        self,
        line_parser: Callable,
        line_preprocessor: LinePreprocessor = None,
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
            block_parser=self.process_block,
            instrument_provider=instrument_provider,
        )
        self.line_preprocessor = line_preprocessor or LinePreprocessor()
        if not PY37:
            assert inspect.isgeneratorfunction(self.line_preprocessor.__call__)

    def parse(self, block) -> Generator:  # noqa: C901
        self.buffer += block
        if b"\n" in block:
            process, self.buffer = self.buffer.rsplit(b"\n", maxsplit=1)
        else:
            process, self.buffer = block, b""
        if process:
            yield from self.process_block(block=process)

    def process_block(self, block: bytes):
        assert isinstance(block, bytes), "Block not bytes"
        for raw_line in block.split(b"\n"):
            gen = self.line_preprocessor(raw_line)
            line = next(gen)
            if not line:
                continue
            instruments = self.check_instrument_provider(data=line)
            if instruments:
                yield from instruments
            objects = tuple(self.parser(line))
            for obj in objects:
                gen.send(obj)
                obj = next(gen)
                yield obj


class CSVReader(Reader):
    """
    Provides parsing of CSV formatted bytes strings to Nautilus objects.
    """

    def __init__(
        self,
        block_parser: Callable,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update=None,
        chunked=True,
        as_dataframe=False,
    ):
        """
        Initialize a new instance of the ``CSVReader`` class.

        Parameters
        ----------
        block_parser : callable
            The handler which takes byte strings and yields Nautilus objects.
        instrument_provider_update
            Optional hook to call before `parser` for the purpose of loading instruments into an InstrumentProvider
        chunked: bool, default=True
            If chunked=False, each CSV line will be passed to `block_parser` individually, if chunked=True, the data
            passed will potentially contain many lines (a block).
        as_dataframe: bool, default=False
            If as_dataframe=True, the passes block will be parsed into a DataFrame before passing to `block_parser`

        """
        super().__init__(
            instrument_provider=instrument_provider,
            instrument_provider_update=instrument_provider_update,
        )
        self.block_parser = block_parser
        self.header: Optional[List[str]] = None
        self.chunked = chunked
        self.as_dataframe = as_dataframe

    def parse(self, block: bytes) -> Generator:
        if self.header is None:
            header, block = block.split(b"\n", maxsplit=1)
            self.header = header.decode().split(",")

        self.buffer += block
        process, self.buffer = self.buffer.rsplit(b"\n", maxsplit=1)

        if self.as_dataframe:
            process = pd.read_csv(BytesIO(process), names=self.header)
        if self.instrument_provider_update is not None:
            self.instrument_provider_update(self.instrument_provider, process)
        yield from self.block_parser(process)


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
            block_parser=parser,
            instrument_provider_update=instrument_provider_update,
            instrument_provider=instrument_provider,
        )
        self.parser = parser
        self.filename = None
        self.data_type = data_type

    def parse(self, block: bytes) -> Generator:
        df = pd.read_parquet(BytesIO(block))
        if self.instrument_provider_update is not None:
            self.instrument_provider_update(
                instrument_provider=self.instrument_provider,
                df=df,
                filename=self.filename,
            )
        yield from self.parser(data_type=self.data_type, df=df, filename=self.filename)
