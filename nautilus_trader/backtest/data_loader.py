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
from pyarrow import ArrowInvalid
import pyarrow.dataset as ds
import pyarrow.parquet as pq

from nautilus_trader.model.data import Data
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import GenericData


try:
    from tqdm import tqdm
except ImportError:
    pass

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.orderbook.book import OrderBookDelta
from nautilus_trader.model.orderbook.book import OrderBookDeltas
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from nautilus_trader.model.venue import InstrumentStatusUpdate
from nautilus_trader.serialization.arrow.core import _deserialize
from nautilus_trader.serialization.arrow.core import _partition_keys
from nautilus_trader.serialization.arrow.core import _schemas
from nautilus_trader.serialization.arrow.core import _serialize


NewFile = namedtuple("NewFile", "name")
EOStream = namedtuple("EOStream", "")
GENERIC_DATA_PREFIX = "genericdata_"
category_attributes = {
    "TradeTick": ["instrument_id", "type", "aggressor_side"],
    "OrderBookDelta": ["instrument_id", "type", "level", "delta_type", "order_size"],
}
NAUTILUS_TS_COLUMNS = ("ts_event_ns", "ts_recv_ns", "timestamp_ns")


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
                        self.state, x = x
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
        parser: callable = None,
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
        file_filter: callable = None,
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

    def import_from_data_loader(
        self, loader: DataLoader, append_only=False, progress=False, **kwargs
    ):
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
        for chunk in loader.run(progress=progress):
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

    @staticmethod
    def _determine_partition_cols(cls, instrument_id):
        if _partition_keys.get(cls.__name__) is not None:
            return list(_partition_keys[cls.__name__])
        elif instrument_id is not None:
            return ["instrument_id"]
        return

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
                name = f"{class_to_filename(cls)}.parquet"
                fn = self.root.joinpath(name)

                df = pd.DataFrame(tables[cls][ins_id])

                if df.empty:
                    continue

                ts_col = None
                for c in NAUTILUS_TS_COLUMNS:
                    if c in df.columns:
                        ts_col = c
                assert ts_col is not None, f"Could not find timestamp column for type: {cls}"

                partition_cols = self._determine_partition_cols(cls=cls, instrument_id=ins_id)

                # Load any existing data, drop dupes
                if not append_only:
                    if self.fs.exists(fn) or self.fs.isdir(str(fn)):
                        existing = self._query(
                            filename=camel_to_snake_case(cls.__name__),
                            instrument_ids=ins_id
                            if cls not in Instrument.__subclasses__()
                            else None,
                            ts_column=ts_col,
                            raise_on_empty=False,
                        )
                        if not existing.empty:
                            df = df.append(existing).drop_duplicates()
                            # Remove this file/partition, will be written again
                            if partition_cols:
                                assert partition_cols == [
                                    "instrument_id"
                                ], "Only support appending to instrument_id partitions"
                                # We only want to remove this partition
                                partition_path = f"/instrument_id={clean_key(ins_id)}"
                                self.fs.rm(str(fn) + partition_path, recursive=True)
                            else:
                                self.fs.rm(str(fn), recursive=True)

                df = df.sort_values(ts_col)
                df = df.astype({k: "category" for k in category_attributes.get(cls.__name__, [])})
                for col in NAUTILUS_TS_COLUMNS:
                    if col in df.columns:
                        df = df.sort_values(col)
                        break

                df, mappings = clean_partition_cols(df, partition_cols)
                schema = _schemas.get(cls.__name__)
                table = pa.Table.from_pandas(df, schema=schema)
                metadata_collector = []
                pq.write_to_dataset(
                    table=table,
                    root_path=str(fn),
                    filesystem=self.fs,
                    partition_cols=partition_cols,
                    # use_legacy_dataset=True,
                    version="2.0",
                    metadata_collector=metadata_collector,
                    **kwargs,
                )
                # Write the ``_common_metadata`` parquet file without row groups statistics
                pq.write_metadata(table.schema, fn / "_common_metadata", version="2.0")

                # Write the ``_metadata`` parquet file with row groups statistics of all files
                pq.write_metadata(table.schema, fn / "_metadata", version="2.0")

                # Write out any partition columns we had to modify due to filesystem requirements
                if mappings:
                    self._write_mappings(fn=fn, mappings=mappings)

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
                    engine.add_trade_tick_objects(instrument_id=instrument.id, data=data[name])
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
                {},
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
                    raise_on_empty=False,
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
        raise_on_empty=True,
    ):
        filters = [filter_expr] if filter_expr is not None else []
        if instrument_ids is not None:
            if not isinstance(instrument_ids, list):
                instrument_ids = [instrument_ids]
            filters.append(
                ds.field("instrument_id").isin(list(set(map(clean_key, instrument_ids))))
            )
        if start is not None:
            filters.append(ds.field(ts_column) >= int(pd.Timestamp(start).to_datetime64()))
        if end is not None:
            filters.append(ds.field(ts_column) <= int(pd.Timestamp(end).to_datetime64()))

        path = f"{self.root}/{filename}.parquet/"
        if not self.fs.exists(path):
            if raise_on_empty:
                raise FileNotFoundError
            else:
                return pd.DataFrame()

        dataset = ds.dataset(path, partitioning="hive", filesystem=self.fs)
        table = dataset.to_table(filter=combine_filters(*filters))
        df = table.to_pandas().drop_duplicates()
        mappings = self._read_mappings(path=path)
        for col in mappings:
            df.loc[:, col] = df[col].map({v: k for k, v in mappings[col].items()})

        # TODO (bm) - This should be stored as a dictionary (category) anyway.
        if "instrument_id" in df.columns:
            df = df.astype({"instrument_id": "category"})
        if df.empty and raise_on_empty:
            local_vars = dict(locals())
            kw = [
                f"{k}={local_vars[k]}"
                for k in ("filename", "filter_expr", "instrument_ids", "start", "end")
            ]
            raise ValueError(f"Data empty for {kw}")
        return df

    def _write_mappings(self, fn, mappings):
        with self.fs.open(fn / "_partition_mappings.json", "wb") as f:
            f.write(orjson.dumps(mappings))

    def _read_mappings(self, path):
        try:
            with self.fs.open(path + "_partition_mappings.json") as f:
                return orjson.loads(f.read())
        except FileNotFoundError:
            return {}

    @staticmethod
    def _make_objects(df, cls):
        if df is None:
            return []
        return _deserialize(cls=cls, chunk=df.to_dict("records"))

    def instruments(
        self,
        instrument_type=None,
        instrument_ids=None,
        filter_expr=None,
        as_nautilus=False,
        **kwargs,
    ):
        if instrument_type is not None:
            assert isinstance(instrument_type, type)
            instrument_types = (instrument_type,)
        else:
            instrument_types = Instrument.__subclasses__()

        dfs = []
        for ins_type in instrument_types:
            try:
                df = self._query(
                    camel_to_snake_case(ins_type.__name__),
                    filter_expr=filter_expr,
                    instrument_ids=instrument_ids,
                    raise_on_empty=False,
                    **kwargs,
                )
                df = df.drop_duplicates(
                    [c for c in df.columns if c not in NAUTILUS_TS_COLUMNS], keep="last"
                )
                dfs.append(df)
            except ArrowInvalid as e:
                # If we're using a `filter_expr` here, there's a good chance this error is using a filter that is
                # specific to one set of instruments and not the others, so we ignore it. If not; raise
                if filter_expr is not None:
                    continue
                else:
                    raise e

        if not as_nautilus:
            return pd.concat([df for df in dfs if df is not None])
        else:
            objects = []
            for ins_type, df in zip(instrument_types, dfs):
                if df is None or (isinstance(df, pd.DataFrame) and df.empty):
                    continue
                objects.extend(self._make_objects(df=df, cls=ins_type))
            return objects

    def instrument_status_events(
        self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs
    ):
        df = self._query(
            "instrument_status_update",
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df
        return self._make_objects(df=df, cls=InstrumentStatusUpdate)

    def trade_ticks(self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs):
        df = self._query(
            "trade_tick",
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df.astype({"price": float, "size": float})
        return self._make_objects(df=df, cls=TradeTick)

    def quote_ticks(self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs):
        df = self._query(
            "quote_tick",
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df
        return self._make_objects(df=df, cls=QuoteTick)

    def order_book_deltas(self, instrument_ids=None, filter_expr=None, as_nautilus=False, **kwargs):
        df = self._query(
            "order_book_delta",
            instrument_ids=instrument_ids,
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df
        return self._make_objects(df=df, cls=OrderBookDeltas)

    def generic_data(self, cls, filter_expr=None, as_nautilus=False, **kwargs):
        df = self._query(
            filename=class_to_filename(cls),
            filter_expr=filter_expr,
            **kwargs,
        )
        if not as_nautilus:
            return df
        return [
            GenericData(data_type=DataType(cls), data=d) for d in self._make_objects(df=df, cls=cls)
        ]

    def query(self, cls, filter_expr=None, instrument_ids=None, as_nautilus=False, **kwargs):
        name = class_to_filename(cls)
        if name.startswith(GENERIC_DATA_PREFIX):
            # Special handling for generic data
            return self.generic_data(
                cls=cls,
                filter_expr=filter_expr,
                instrument_ids=instrument_ids,
                as_nautilus=as_nautilus,
                **kwargs,
            )
        df = self._query(
            filename=name,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            **kwargs,
        )
        if not as_nautilus:
            return df
        return self._make_objects(df=df, cls=cls)

    def list_data_types(self):
        return [p.stem for p in self.root.glob("*.parquet")]

    def list_generic_data_types(self):
        data_types = self.list_data_types()
        return [
            n.replace(GENERIC_DATA_PREFIX, "")
            for n in data_types
            if n.startswith(GENERIC_DATA_PREFIX)
        ]

    def list_partitions(self, cls_type):
        assert isinstance(cls_type, type), "`cls_type` should be type, ie TradeTick"
        prefix = GENERIC_DATA_PREFIX if is_custom_data(cls_type) else ""
        name = prefix + camel_to_snake_case(cls_type.__name__)
        dataset = pq.ParquetDataset(self.root / f"{name}.parquet")
        partitions = {}
        for level in dataset.partitions.levels:
            partitions[level.name] = level.keys
        return partitions


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
    is_nautilus_paths = cls.__module__.startswith("nautilus_trader.")
    if not is_nautilus_paths:
        # This object is defined outside of nautilus, definitely custom
        return True
    else:
        is_data_subclass = issubclass(cls, Data)
        is_nautilus_builtin = any(
            (cls.__module__.startswith(p) for p in ("nautilus_trader.model",))
        )
        return is_data_subclass and not is_nautilus_builtin


def identity(x):
    return x


INVALID_WINDOWS_CHARS = r'<>:"/\|?* '


def clean_partition_cols(df, partition_cols=None):
    mappings = {}
    for col in partition_cols or []:
        values = list(map(str, df[col].unique()))
        invalid_values = {val for val in values if any(x in val for x in INVALID_WINDOWS_CHARS)}
        if invalid_values:
            if col == "instrument_id":
                # We have control over how instrument_ids are retrieved from the cache, so we can do this replacement
                val_map = {k: clean_key(k) for k in values}
                mappings[col] = val_map
                df.loc[:, col] = df[col].map(val_map)

            else:
                # We would be arbitrarily replacing values here which could break queries, we should not do this.
                raise ValueError(
                    f"Some values in partition column [{col}] contain invalid characters: {invalid_values}"
                )
    return df, mappings


def clean_key(s):
    for ch in INVALID_WINDOWS_CHARS:
        if ch in s:
            s = s.replace(ch, "-")
    return s


def class_to_filename(cls):
    name = f"{camel_to_snake_case(cls.__name__)}"
    if is_custom_data(cls):
        name = f"{GENERIC_DATA_PREFIX}{camel_to_snake_case(cls.__name__)}"
    return name


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
