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
from collections import namedtuple
from io import BytesIO
from typing import Callable, Generator

import pandas as pd

from nautilus_trader.serialization.arrow.util import identity


NewFile = namedtuple("NewFile", "name")
EOStream = namedtuple("EOStream", "")


class ByteParser:
    """
    The base class for all byte string parsers.
    """

    def __init__(
        self,
        instrument_provider_update: Callable = None,
    ):
        """
        Initialize a new instance of the ``ByteParser`` class.
        """
        self.instrument_provider_update = instrument_provider_update

    def read(self, stream: Generator, instrument_provider=None) -> Generator:
        raise NotImplementedError


class TextParser(ByteParser):
    """
    Provides parsing of byte strings to Nautilus objects.
    """

    def __init__(
        self,
        parser: Callable,
        line_preprocessor: Callable = None,
        instrument_provider_update: Callable = None,
    ):
        """
        Initialize a new instance of the ``TextParser`` class.

        Parameters
        ----------
        parser : Callable
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
        super().__init__(instrument_provider_update=instrument_provider_update)

        self.parser = parser
        self.line_preprocessor = line_preprocessor or identity
        self.state = None

    def on_new_file(self, new_file):
        pass

    def read(self, stream: Generator, instrument_provider=None) -> Generator:  # noqa: C901
        raw = b""
        fn = None
        for chunk in stream:
            if isinstance(chunk, NewFile):
                # New file, yield any remaining data, reset bytes
                assert not raw, f"Data remaining at end of file: {fn}"
                fn = chunk.name
                raw = b""
                self.on_new_file(chunk)
                yield chunk
                continue
            elif isinstance(chunk, EOStream):
                yield chunk
                return
            elif chunk is None:
                yield
                continue
            if chunk == b"":
                # This is probably EOF? Append a newline to ensure we emit the previous line
                chunk = b"\n"
            raw += chunk
            process, raw = raw.rsplit(b"\n", maxsplit=1)
            if process:
                for x in self.process_chunk(chunk=process, instrument_provider=instrument_provider):
                    try:
                        x, self.state = x
                    except TypeError as e:
                        if not e.args[0].startswith("cannot unpack non-iterable"):
                            raise e
                    yield x

    def process_chunk(self, chunk, instrument_provider):
        for line in map(self.line_preprocessor, chunk.split(b"\n")):
            if self.instrument_provider_update is not None:
                # Check the user hasn't accidentally used a generator here also
                r = self.instrument_provider_update(instrument_provider, line)
                if isinstance(r, Generator):
                    raise Exception(
                        f"{self.instrument_provider_update} func should not be generator"
                    )
            yield from self.parser(line, state=self.state)


class CSVParser(TextParser):
    """
    Provides parsing of CSV formatted bytes strings to Nautilus objects.
    """

    def __init__(
        self,
        parser: Callable,
        line_preprocessor=None,
        instrument_provider_update=None,
    ):
        """
        Initialize a new instance of the ``CSVParser`` class.

        Parameters
        ----------
        parser : callable
            The handler which takes byte strings and yields Nautilus objects.
        line_preprocessor : callable
            Optional handler to clean log lines prior to processing by `parser`
        instrument_provider_update
            Optional hook to call before `parser` for the purpose of loading instruments into an InstrumentProvider

        """
        super().__init__(
            parser=parser,
            line_preprocessor=line_preprocessor,
            instrument_provider_update=instrument_provider_update,
        )
        self.header = None

    def on_new_file(self, new_file):
        self.header = None

    def process_chunk(self, chunk, instrument_provider):
        if self.header is None:
            header, chunk = chunk.split(b"\n", maxsplit=1)
            self.header = header.decode().split(",")
        df = pd.read_csv(BytesIO(chunk), names=self.header)
        if self.instrument_provider_update is not None:
            self.instrument_provider_update(instrument_provider, df)
        yield from self.parser(df, state=self.state)


class ParquetParser(ByteParser):
    """
    Provides parsing of parquet specification bytes to Nautilus objects.
    """

    def __init__(
        self,
        data_type: str,
        parser: Callable = None,
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
            instrument_provider_update=instrument_provider_update,
        )
        self.parser = parser
        self.filename = None
        self.data_type = data_type

    def read(self, stream: Generator, instrument_provider=None) -> Generator:
        for chunk in stream:
            if isinstance(chunk, NewFile):
                self.filename = chunk.name
                if self.instrument_provider_update is not None:
                    self.instrument_provider_update(instrument_provider, chunk)
                yield chunk
            elif isinstance(chunk, EOStream):
                self.filename = None
                yield chunk
                return
            elif chunk is None:
                yield
            elif isinstance(chunk, bytes):
                if len(chunk):
                    df = pd.read_parquet(BytesIO(chunk))
                    if self.instrument_provider_update is not None:
                        self.instrument_provider_update(
                            instrument_provider=instrument_provider,
                            df=df,
                            filename=self.filename,
                        )
                    yield from self.parser(data_type=self.data_type, df=df, filename=self.filename)
            else:
                raise TypeError
