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

from collections import defaultdict
from collections import namedtuple
from io import BytesIO
from itertools import takewhile
import os
import pathlib
import re
from typing import Generator
import warnings

import fsspec
import orjson
import pandas as pd
import pyarrow as pa
import pyarrow.dataset as ds
import pyarrow.parquet as pq

from nautilus_trader.data.wrangling import QuoteTickDataWrangler
from nautilus_trader.data.wrangling import TradeTickDataWrangler
from nautilus_trader.model.data import Data


try:
    from tqdm import tqdm
except ImportError:
    pass

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.model.events import InstrumentStatusEvent
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.orderbook.book import OrderBookDelta
from nautilus_trader.model.orderbook.book import OrderBookDeltas
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from nautilus_trader.serialization.arrow.core import _deserialize
from nautilus_trader.serialization.arrow.core import _serialize


NewFile = namedtuple("NewFile", "name")
EOStream = namedtuple("EOStream", "")
GENERIC_DATA_PREFIX = "genericdata_"
category_attributes = {
    "TradeTick": ["instrument_id", "type", "aggressor_side"],
    "OrderBookDelta": ["instrument_id", "type", "level", "delta_type", "order_size"],
}


class ByteParser:
    """
    The base class for all byte string parsers.
    """

    def __init__(
        self,
        instrument_provider_update: callable = None,
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
        parser: callable,
        line_preprocessor: callable = None,
        instrument_provider_update: callable = None,
    ):
        """
        Initialize a new instance of the ``TextParser`` class.

        Parameters
        ----------
        parser : callable
            The handler which takes byte strings and yields Nautilus objects.
        line_preprocessor : callable, optional
            The context manager for preprocessing (cleaning log lines) of lines
            before json.loads is called. Nautilus objects are returned to the
            context manager for any post-processing also (for example, setting
            the `ts_recv_ns`).
        instrument_provider_update : callable , optional
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

    def read(  # noqa: C901
        self, stream: Generator, instrument_provider=None
    ) -> Generator:
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
            elif chunk is EOStream:
                yield EOStream
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
                for x in self.process_chunk(
                    chunk=process, instrument_provider=instrument_provider
                ):
                    try:
                        self.state, x = x
                    except TypeError as e:
                        if not e.args[0].startswith("cannot unpack non-iterable"):
                            raise e
                        yield x

    def process_chunk(self, chunk, instrument_provider):
        for line in map(self.line_preprocessor, chunk.split(b"\n")):
            if self.instrument_provider_update is not None:
                self.instrument_provider_update(instrument_provider, line)
            yield from self.parser(line, state=self.state)


class CSVParser(TextParser):
    """
    Provides parsing of CSV formatted bytes strings to Nautilus objects.
    """

    def __init__(
        self,
        parser: callable,
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
        yield from self.parser(df, state=self.state)


class ParquetParser(ByteParser):
    """
    Provides parsing of parquet specification bytes to Nautilus objects.
    """

    def __init__(
        self,
        data_type: str,
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
        self.filename = None
        self.data_type = data_type

    def parse(self, df):
        if self.data_type == "quote_ticks":
            wrangler = QuoteTickDataWrangler(df=df)
        elif self.data_type == "trade_ticks":
            wrangler = TradeTickDataWrangler(df=df)
        else:
            raise TypeError()
        return wrangler.build_ticks()

    def read(self, stream: Generator, instrument_provider=None) -> Generator:
        for chunk in stream:
            if isinstance(chunk, NewFile):
                yield chunk
            elif chunk == EOStream:
                yield chunk
                return
            elif chunk is None:
                yield
            elif isinstance(chunk, bytes):
                if len(chunk):
                    df = pd.read_parquet(BytesIO(chunk))
                    yield from self.parse(df)
            else:
                raise TypeError


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
        compression="infer",
        instrument_provider=None,
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
        compression : bool
            If compression is used. Defaults to 'infer' by file extension.
        instrument_provider : InstrumentProvider
            The instrument provider for the loader.

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
        self.path = sorted(self._resolve_path(path=self._path))

    def _resolve_path(self, path: str):
        if self.fs.isfile(path):
            return [path]
        # We have a directory
        if not path.endswith("/"):
            path += "/"
        if self.fs.isdir(path):
            files = self.fs.glob(f"{path}{self.glob_pattern}")
            assert (
                files
            ), f"Found no files with path={str(path)}, glob={self.glob_pattern}"
            return [f for f in files if self.fs.isfile(f)]
        else:
            raise ValueError("path argument must be str and a valid directory or file")

    def stream_bytes(self, progress=False):
        path = self.path if not progress else tqdm(self.path)
        for fn in path:
            with fsspec.open(fn, compression=self.compression) as f:
                yield NewFile(fn)
                data = 1
                while data:
                    data = f.read(self.chunk_size)
                    yield data
                    yield None  # this is a chunk
        yield EOStream

    def run(self, progress=False):
        stream = self.parser.read(
            stream=self.stream_bytes(progress=progress),
            instrument_provider=self.instrument_provider,
        )
        while 1:
            chunk = list(takewhile(lambda x: x is not None, stream))
            if chunk == [EOStream]:
                break
            if not chunk:
                continue
            # TODO (bm): shithacks - We need a better way to generate instruments?
            if self.instrument_provider is not None:
                # Find any instrument status updates, if we have some, emit instruments first
                instruments = [
                    self.instrument_provider.find(s.instrument_id)
                    for s in chunk
                    if isinstance(s, InstrumentStatusEvent)
                ]
                chunk = instruments + chunk
            yield chunk


class DataCatalog:
    """
    Provides a searchable data catalogue.
    """

    def __init__(self, path=None, fs_protocol=None):
        """
        Initialize a new instance of the ``DataCatalog`` class.

        Parameters
        ----------
        path : str
            The root path to the data.
        fs_protocol : str
            The file system protocol to use.

        """
        self.fs = fsspec.filesystem(
            fs_protocol or os.environ.get("NAUTILUS_BACKTEST_FS_PROTOCOL", "file")
        )
        self.root = pathlib.Path(path or os.environ["NAUTILUS_BACKTEST_DIR"])
        self._processed_files_fn = f"{self.root}/.processed_raw_files.json"

    # ---- Loading data ---------------------------------------------------------------------------------------- #

    def import_from_data_loader(self, loader: DataLoader, append_only=False, **kwargs):
        """
        Load data from a DataLoader instance into the backtest catalogue.

        Parameters
        ----------
        loader : DataLoader
            The data loader to use.
        append_only : bool
            If read existing data to dedupe + sort.
            Use this if the data is strictly ordered.
        kwargs : dict
            The kwargs passed through to `ParquetWriter`.

        """
        for chunk in loader.run(progress=kwargs.pop("progress", False)):
            self._write_chunks(chunk=chunk, append_only=append_only, **kwargs)

    def _save_processed_raw_files(self, files):
        # TODO(bm): We should save a hash of the contents alongside the filename to check for changes
        # load existing
        existing = self._load_processed_raw_files()
        new = set(files + existing)
        with self.fs.open(self._processed_files_fn, "wb") as f:
            return f.write(orjson.dumps(sorted(new)))

    def _load_processed_raw_files(self):
        if self.fs.exists(self._processed_files_fn):
            with self.fs.open(self._processed_files_fn, "rb") as f:
                return orjson.loads(f.read())
        else:
            return []

    def _write_chunks(self, chunk, append_only=False, **kwargs):  # noqa: C901
        processed_raw_files = self._load_processed_raw_files()
        log_filenames = kwargs.pop("log_filenames", False)

        # Split objects into their respective tables
        type_conv = {OrderBookDeltas: OrderBookDelta, OrderBookSnapshot: OrderBookDelta}
        tables = defaultdict(dict)
        skip_file = False
        for obj in chunk:
            if skip_file:
                continue
            if isinstance(obj, NewFile):
                if log_filenames:
                    print(obj.name)
                if obj.name in processed_raw_files:
                    skip_file = True
                else:
                    skip_file = False
                    processed_raw_files.append(obj.name)
                continue

            # TODO (bm) - better handling of instruments -> currency we're writing a file per instrument
            cls = type_conv.get(type(obj), type(obj))
            for data in maybe_list(_serialize(obj)):
                instrument_id = data.get("instrument_id", None)
                if instrument_id not in tables[cls]:
                    tables[cls][instrument_id] = []
                tables[cls][instrument_id].append(data)

        for cls in tables:
            for ins_id in tables[cls]:
                name = f"{camel_to_snake_case(cls.__name__)}.parquet"
                if is_custom_data(cls):
                    name = f"{GENERIC_DATA_PREFIX}{camel_to_snake_case(cls.__name__)}.parquet"
                fn = self.root.joinpath(name)

                df = pd.DataFrame(tables[cls][ins_id])

                if df.empty:
                    continue

                # Load any existing data, drop dupes
                if not append_only:
                    if self.fs.exists(fn):
                        existing = pd.read_parquet(
                            str(fn),
                            fs=self.fs,
                            filters=[("instrument_id", "=", ins_id)],
                        )
                        df = df.append(existing).drop_duplicates()
                        # Remove file, will be written again
                        self.fs.rm(fn, recursive=True)

                df = df.astype(
                    {k: "category" for k in category_attributes.get(cls.__name__, [])}
                )
                for col in ("ts_event_ns", "ts_recv_ns", "timestamp_ns"):
                    if col in df.columns:
                        df = df.sort_values(col)
                        break
                table = pa.Table.from_pandas(df)

                metadata_collector = []
                pq.write_to_dataset(
                    table=table,
                    root_path=str(fn),
                    filesystem=self.fs,
                    partition_cols=["instrument_id"] if ins_id is not None else None,
                    # use_legacy_dataset=True,
                    version="2.0",
                    metadata_collector=metadata_collector,
                    **kwargs,
                )
                # Write the ``_common_metadata`` parquet file without row groups statistics
                pq.write_metadata(table.schema, fn / "_common_metadata", version="2.0")

                # Write the ``_metadata`` parquet file with row groups statistics of all files
                pq.write_metadata(table.schema, fn / "_metadata", version="2.0")

            # Save any new processed files
        self._save_processed_raw_files(files=processed_raw_files)

    def clear_cache(self, **kwargs):
        force = kwargs.get("FORCE", False)
        if not force:
            print(
                "Are you sure you want to clear the WHOLE BACKTEST CACHE?, if so, call clear_cache(FORCE=True)"
            )
        else:
            self.fs.rm(self.root, recursive=True)

    # ---- BACKTEST ---------------------------------------------------------------------------------------- #

    def setup_engine(
        self,
        engine: BacktestEngine,
        instruments,
        chunk_size=None,
        **kwargs,
    ) -> BacktestEngine:
        """
        Load data into a backtest engine.

        Parameters
        ----------
        engine : BacktestEngine
            The backtest engine to load data into.
        instruments : list[Instrument]
            The instruments to load data for.
        chunk_size : int
            The chunk size to return (used for streaming backtest).
            Use None for a loading all the data.
        kwargs : dict
            The kwargs passed to `self.load_backtest_data`.

        """
        data = self.load_backtest_data(
            instrument_ids=[ins.id.value for ins in instruments],
            chunk_size=chunk_size,
            **kwargs,
        )

        # TODO(bm): Handle chunk size
        if chunk_size is not None:
            pass

        # Add instruments & data to engine
        for instrument in instruments:
            engine.add_instrument(instrument)
            for name in data:
                if name == "trade_ticks":
                    engine.add_trade_tick_objects(
                        instrument_id=instrument.id, data=data[name]
                    )
                elif name == "quote_ticks":
                    engine.add_quote_ticks(instrument_id=instrument.id, data=data[name])
                elif name == "order_book_deltas":
                    engine.add_order_book_data(data=data[name])
                # TODO currently broken - BacktestEngine needs to accept events
                # elif name == "instrument_status_events":
                #     engine.add_other_data(data=data[name])

        return engine

    # ---- QUERIES ---------------------------------------------------------------------------------------- #

    # def _load_chunked_backtest_data(self, name, query, instrument_ids, filters, chunk_size):
    #     """
    #     Stream chunked data from parquet dataset
    #
    #     :param name:
    #     :param query:
    #     :param instrument_ids:
    #     :param filters:
    #     :return:
    #     """
    #     # TODO - look at dask.dataframe.aggregate_row_groups for chunking solution
    #     dataset = query(instrument_ids=instrument_ids, filters=filters, return_dataset=True)
    #     ts_column_idx = ds.schema.names.index('ts_recv_ns')
    #     for piece in ds.pieces:
    #         meta = piece.get_metadata()
    #         for i in range(meta.num_row_groups):
    #             rg = meta.row_group(i)
    #             rg_size = rg.total_byte_size
    #             ts_stats = rg.column(ts_column_idx).statistics
    #     return

    def load_backtest_data(
        self,
        instrument_ids=None,
        start_timestamp=None,
        end_timestamp=None,
        order_book_deltas=True,
        trade_ticks=True,
        quote_ticks=False,
        instrument_status_events=True,
        chunk_size=None,
    ):
        """
        Load backtest data objects from the catalogue.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instruments to load data for.
        start_timestamp : datetime
            The starting timestamp of the data to load.
        end_timestamp : datetime
            The ending timestamp of the data to load.
        order_book_deltas : bool
            If order book deltas should be loaded.
        trade_ticks : bool
            If trade ticks should be loaded.
        quote_ticks : bool
            If quote ticks should be loaded.
        instrument_status_events : bool
            If instrument status events should be loaded.
        chunk_size : int
            The chunk size to return (used for streaming backtest).
            Use None for a loading all the data.

        """
        assert instrument_ids is None or isinstance(
            instrument_ids, list
        ), "instrument_ids must be list"
        queries = [
            ("order_book_deltas", order_book_deltas, self.order_book_deltas, {}),
            ("trade_ticks", trade_ticks, self.trade_ticks, {}),
            (
                "instrument_status_events",
                instrument_status_events,
                self.instrument_status_events,
                {"ts_column": "timestamp_ns"},
            ),
            ("quote_ticks", quote_ticks, self.quote_ticks, {}),
        ]
        data = {}

        if chunk_size:
            raise KeyError
            # data[name] = self._load_chunked_backtest_data(
            #     chunk_size=chunk_size, name=name, query=query, instrument_ids=instrument_ids, filters=filters,
            # )

        for name, to_load, query, kw in queries:
            if to_load:
                data[name] = query(
                    instrument_ids=instrument_ids,
                    as_nautilus=True,
                    start=start_timestamp,
                    end=end_timestamp,
                    **kw,
                )

        return data

    def _query(
        self,
        filename,
        filter_expr=None,
        instrument_ids=None,
        start=None,
        end=None,
        ts_column="ts_event_ns",
    ):
        filters = [filter_expr] if filter_expr is not None else []
        if instrument_ids is not None:
            if not isinstance(instrument_ids, list):
                instrument_ids = [instrument_ids]
            filters.append(ds.field("instrument_id").isin(list(set(instrument_ids))))
        if start is not None:
            filters.append(ds.field(ts_column) >= start)
        if end is not None:
            filters.append(ds.field(ts_column) <= end)

        dataset = ds.dataset(
            f"{self.root}/{filename}.parquet/",
            partitioning="hive",
            filesystem=self.fs,
        )
        df = (
            dataset.to_table(filter=combine_filters(*filters))
            .to_pandas()
            .drop_duplicates()
        )
        if "instrument_id" in df.columns:
            df = df.astype({"instrument_id": "category"})
        return df

    @staticmethod
    def _make_objects(df, cls):
        return _deserialize(name=cls, chunk=df.to_dict("records"))

    def instruments(self, filter_expr=None, as_nautilus=False, **kwargs):
        df = self._query("betting_instrument", filter_expr=filter_expr, **kwargs)
        if not as_nautilus:
            return df
        return self._make_objects(df=df.drop(["type"], axis=1), cls=BettingInstrument)

    def instrument_status_events(
        self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs
    ):
        df = self._query(
            "instrument_status_event",
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df
        return self._make_objects(df=df, cls=InstrumentStatusEvent)

    def trade_ticks(
        self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs
    ):
        df = self._query(
            "trade_tick",
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df.astype({"price": float, "size": float})
        return self._make_objects(df=df, cls=TradeTick)

    def quote_ticks(
        self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs
    ):
        df = self._query(
            "quote_tick",
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df
        return self._make_objects(df=df, cls=QuoteTick)

    def order_book_deltas(
        self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs
    ):
        df = self._query(
            "order_book_delta",
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df
        return self._make_objects(df=df, cls=OrderBookDelta)

    def generic_data(self, name, filter_expr=None, as_nautilus=False, **kwargs):
        df = self._query(
            f"{GENERIC_DATA_PREFIX}{name}",
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df
        return self._make_objects(df=df, cls=OrderBookDelta)


def camel_to_snake_case(s):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", s).lower()


def parse_timestamp(t):
    return int(pd.Timestamp(t).timestamp() * 1e9)


def maybe_list(obj):
    if isinstance(obj, dict):
        return [obj]
    return obj


def combine_filters(*filters):
    filters = tuple(x for x in filters if x is not None)
    if len(filters) == 0:
        return
    elif len(filters) == 1:
        return filters[0]
    else:
        expr = filters[0]
        for f in filters[1:]:
            expr = expr & f
        return expr


def is_custom_data(cls):
    builtin_paths = ("nautilus_trader.models",)
    return cls in Data.__subclasses__() and not any(
        (cls.__name__.startswith(p) for p in builtin_paths)
    )


def identity(x):
    return x


# TODO - https://github.com/leonidessaguisagjr/filehash ?
# def get_digest(fs, path):
#     h = hashlib.sha256()
#
#     with fs.open(path, 'rb') as file:
#         while True:
#             # Reading is buffered, so we can read smaller chunks.
#             chunk = file.read(h.block_size)
#             if not chunk:
#                 break
#             h.update(chunk)
#
#     return h.hexdigest()
