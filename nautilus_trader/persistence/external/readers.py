# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
import logging
from io import BytesIO
from typing import Any, Callable, Dict, Generator, List, Optional, Union

import pandas as pd

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.instruments.base import Instrument


class LinePreprocessor:
    """
    Provides pre-processing lines before they are passed to a `Reader` class
    (currently only `TextReader`).

    Used if the input data requires any pre-processing that may also be required
    as attributes on the resulting Nautilus objects that are created.

    Examples
    --------
    For example, if you were logging data in Python with a prepended timestamp, as below:

    2021-06-29T06:03:14.528000 - {"op":"mcm","pt":1624946594395,"mc":[{"id":"1.179082386","rc":[{"atb":[[1.93,0]]}]}

    The raw JSON data is contained after the logging timestamp, additionally we would
    also want to use this timestamp as the Nautilus `ts_init` value. In
    this instance, you could use something like:

    >>> class LoggingLinePreprocessor(LinePreprocessor):
    >>>    @staticmethod
    >>>    def pre_process(line):
    >>>        timestamp, json_data = line.split(' - ')
    >>>        yield json_data, {'ts_init': pd.Timestamp(timestamp)}
    >>>
    >>>    @staticmethod
    >>>    def post_process(obj: Any, state: dict):
    >>>        obj.ts_init = state['ts_init']
    >>>        return obj
    """

    def __init__(self):
        self.state = {}
        self.line = None

    @staticmethod
    def pre_process(line: bytes) -> Dict:
        return {"line": line, "state": {}}

    @staticmethod
    def post_process(obj: Any, state: dict) -> Any:
        return obj

    def process_new_line(self, raw_line: bytes):
        result: Dict = self.pre_process(raw_line)
        err = "Return value of `pre_process` should be dict with keys `line` and `state`"
        assert isinstance(result, dict) and "line" in result and "state" in result, err
        self.line = result["line"]
        self.state = result["state"]
        return self.line

    def process_object(self, obj: Any):
        return self.post_process(obj=obj, state=self.state)

    def clear(self):
        self.line = None
        self.state = {}


class Reader:
    """
    Provides parsing of raw byte blocks to Nautilus objects.
    """

    def __init__(
        self,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update: Callable = None,
    ):
        self.instrument_provider = instrument_provider
        self.instrument_provider_update = instrument_provider_update
        self.buffer = b""

    def check_instrument_provider(self, data: Union[bytes, str]) -> List[Instrument]:
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
        return []

    def on_file_complete(self):
        self.buffer = b""

    def parse(self, block: bytes) -> Generator:
        raise NotImplementedError()  # pragma: no cover


class ByteReader(Reader):
    """
    A Reader subclass for reading blocks of raw bytes; `byte_parser` will be
    passed a blocks of raw bytes.

    Parameters
    ----------
    block_parser : Callable
        The handler which takes a blocks of bytes and yields Nautilus objects.
    instrument_provider : InstrumentProvider, optional
        The instrument provider for the reader.
    instrument_provider_update : Callable , optional
        An optional hook/callable to update instrument provider before data is passed to `byte_parser`
        (in many cases instruments need to be known ahead of parsing).
    """

    def __init__(
        self,
        block_parser: Callable,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update: Callable = None,
    ):
        super().__init__(
            instrument_provider_update=instrument_provider_update,
            instrument_provider=instrument_provider,
        )
        assert inspect.isgeneratorfunction(block_parser)
        self.parser = block_parser

    def parse(self, block: bytes) -> Generator:
        instruments: List[Instrument] = self.check_instrument_provider(data=block)
        if instruments:
            yield from instruments
        yield from self.parser(block)


class TextReader(ByteReader):
    """
    A Reader subclass for reading lines of a text-like file; `line_parser` will
    be passed a single row of bytes.

    Parameters
    ----------
    line_parser : Callable
        The handler which takes byte strings and yields Nautilus objects.
    line_preprocessor : Callable, optional
        The context manager for pre-processing (cleaning log lines) of lines
        before json.loads is called. Nautilus objects are returned to the
        context manager for any post-processing also (for example, setting
        the `ts_init`).
    instrument_provider : InstrumentProvider, optional
        The instrument provider for the reader.
    instrument_provider_update : Callable, optional
        An optional hook/callable to update instrument provider before
        data is passed to `line_parser` (in many cases instruments need to
        be known ahead of parsing).
    newline : bytes
        The newline char value.
    """

    def __init__(
        self,
        line_parser: Callable,
        line_preprocessor: LinePreprocessor = None,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update: Optional[Callable] = None,
        newline: bytes = b"\n",
    ):
        assert line_preprocessor is None or isinstance(line_preprocessor, LinePreprocessor)
        super().__init__(
            instrument_provider_update=instrument_provider_update,
            block_parser=line_parser,
            instrument_provider=instrument_provider,
        )
        self.line_preprocessor = line_preprocessor or LinePreprocessor()
        self.newline = newline

    def parse(self, block: bytes) -> Generator:
        self.buffer += block
        if b"\n" in block:
            process, self.buffer = self.buffer.rsplit(self.newline, maxsplit=1)
        else:
            process, self.buffer = block, b""
        if process:
            yield from self.process_block(block=process)

    def process_block(self, block: bytes):
        assert isinstance(block, bytes), "Block not bytes"
        for raw_line in block.split(b"\n"):
            line = self.line_preprocessor.process_new_line(raw_line=raw_line)
            if not line:
                continue
            instruments: List[Instrument] = self.check_instrument_provider(data=line)
            if instruments:
                yield from instruments
            for obj in self.parser(line):
                yield self.line_preprocessor.process_object(obj=obj)
            self.line_preprocessor.clear()


class CSVReader(Reader):
    """
    Provides parsing of CSV formatted bytes strings to Nautilus objects.

    Parameters
    ----------
    block_parser : callable
        The handler which takes byte strings and yields Nautilus objects.
    instrument_provider : InstrumentProvider, optional
        The readers instrument provider.
    instrument_provider_update
        Optional hook to call before `parser` for the purpose of loading instruments into an InstrumentProvider
    header: List[str], default None
        If first row contains names of columns, header has to be set to `None`.
        If data starts right at the first row, header has to be provided the list of column names.
    chunked: bool, default True
        If chunked=False, each CSV line will be passed to `block_parser` individually, if chunked=True, the data
        passed will potentially contain many lines (a block).
    as_dataframe: bool, default False
        If as_dataframe=True, the passes block will be parsed into a DataFrame before passing to `block_parser`.
    """

    def __init__(
        self,
        block_parser: Callable,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update: Optional[Callable] = None,
        header: Optional[List[str]] = None,
        chunked: bool = True,
        as_dataframe: bool = True,
        separator: str = ",",
        newline: bytes = b"\n",
        encoding: str = "utf-8",
    ):
        super().__init__(
            instrument_provider=instrument_provider,
            instrument_provider_update=instrument_provider_update,
        )
        self.block_parser = block_parser
        self.header = header
        self.header_in_first_row = not header
        self.chunked = chunked
        self.as_dataframe = as_dataframe
        self.separator = separator
        self.newline = newline
        self.encoding = encoding

    def parse(self, block: bytes) -> Generator:
        if self.header is None:
            header, block = block.split(b"\n", maxsplit=1)
            self.header = header.decode(self.encoding).split(self.separator)

        self.buffer += block
        if b"\n" in block:
            process, self.buffer = self.buffer.rsplit(self.newline, maxsplit=1)
        else:
            process, self.buffer = block, b""

        # Prepare - a little gross but allows a lot of flexibility
        if self.as_dataframe:
            df = pd.read_csv(BytesIO(process), names=self.header, sep=self.separator)
            if self.chunked:
                chunks = (df,)
            else:
                chunks = tuple([row for _, row in df.iterrows()])  # type: ignore
        else:
            if self.chunked:
                chunks = (process,)
            else:
                chunks = tuple(
                    [
                        dict(zip(self.header, line.split(bytes(self.separator, encoding="utf-8"))))
                        for line in process.split(b"\n")
                    ]
                )  # type: ignore

        for chunk in chunks:
            if self.instrument_provider_update is not None:
                self.instrument_provider_update(self.instrument_provider, chunk)
            yield from self.block_parser(chunk)

    def on_file_complete(self):
        if self.header_in_first_row:
            self.header = None
        self.buffer = b""


class ParquetReader(ByteReader):
    """
    Provides parsing of parquet specification bytes to Nautilus objects.

    Parameters
    ----------
    parser : Callable
        The parser.
    instrument_provider : InstrumentProvider, optional
        The readers instrument provider.
    instrument_provider_update : Callable , optional
        An optional hook/callable to update instrument provider before data is passed to `byte_parser`
        (in many cases instruments need to be known ahead of parsing).
    """

    def __init__(
        self,
        parser: Callable = None,
        instrument_provider: Optional[InstrumentProvider] = None,
        instrument_provider_update: Optional[Callable] = None,
    ):
        super().__init__(
            block_parser=parser,
            instrument_provider_update=instrument_provider_update,
            instrument_provider=instrument_provider,
        )
        self.parser = parser

    def parse(self, block: bytes) -> Generator:
        self.buffer += block
        try:
            df = pd.read_parquet(BytesIO(block))
            self.buffer = b""
        except Exception as e:
            logging.exception(f"Error on parse {block[:128]!r}", e)
            return

        if self.instrument_provider_update is not None:
            self.instrument_provider_update(
                instrument_provider=self.instrument_provider,
                df=df,
            )
        yield from self.parser(df)
