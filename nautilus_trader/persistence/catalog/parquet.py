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
import heapq
import itertools
import os
import pathlib
import platform
import sys
from pathlib import Path
from typing import Callable, Optional, Union

import fsspec
import numpy as np
import pandas as pd
import pyarrow as pa
import pyarrow.dataset as ds
import pyarrow.parquet as pq
from fsspec.implementations.local import make_path_posix
from fsspec.implementations.memory import MemoryFileSystem
from fsspec.utils import infer_storage_options
from pyarrow import ArrowInvalid

from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.objects import FIXED_SCALAR
from nautilus_trader.persistence.catalog.base import BaseDataCatalog
from nautilus_trader.persistence.external.metadata import load_mappings
from nautilus_trader.persistence.external.util import is_filename_in_time_range
from nautilus_trader.persistence.streaming.batching import generate_batches_rust
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.serializer import list_schemas
from nautilus_trader.serialization.arrow.util import camel_to_snake_case
from nautilus_trader.serialization.arrow.util import class_to_filename
from nautilus_trader.serialization.arrow.util import clean_key
from nautilus_trader.serialization.arrow.util import dict_of_lists_to_list_of_dicts


class ParquetDataCatalog(BaseDataCatalog):
    """
    Provides a queryable data catalog persisted to files in parquet format.

    Parameters
    ----------
    path : str
        The root path for this data catalog. Must exist and must be an absolute path.
    fs_protocol : str, default 'file'
        The fsspec filesystem protocol to use.
    fs_storage_options : dict, optional
        The fs storage options.

    Warnings
    --------
    The catalog is not threadsafe.
    """

    def __init__(
        self,
        path: str,
        fs_protocol: Optional[str] = "file",
        fs_storage_options: Optional[dict] = None,
    ):
        self.fs_protocol = fs_protocol
        self.fs_storage_options = fs_storage_options or {}
        self.fs: fsspec.AbstractFileSystem = fsspec.filesystem(
            self.fs_protocol, **self.fs_storage_options
        )

        path = make_path_posix(path)

        if (
            isinstance(self.fs, MemoryFileSystem)
            and platform.system() == "Windows"
            and not path.startswith("/")
        ):
            path = "/" + path

        self.path = str(path)

    @classmethod
    def from_env(cls):
        return cls.from_uri(os.environ["NAUTILUS_PATH"] + "/catalog")

    @classmethod
    def from_uri(cls, uri):
        if "://" not in uri:
            # Assume a local path
            uri = "file://" + uri
        parsed = infer_storage_options(uri)
        path = parsed.pop("path")
        protocol = parsed.pop("protocol")
        storage_options = parsed.copy()
        return cls(path=path, fs_protocol=protocol, fs_storage_options=storage_options)

    # -- QUERIES -----------------------------------------------------------------------------------

    def query(self, cls, filter_expr=None, instrument_ids=None, as_nautilus=False, **kwargs):
        if not is_nautilus_class(cls):
            # Special handling for generic data
            return self.generic_data(
                cls=cls,
                filter_expr=filter_expr,
                instrument_ids=instrument_ids,
                as_nautilus=as_nautilus,
                **kwargs,
            )
        else:
            return self._query(
                cls=cls,
                filter_expr=filter_expr,
                instrument_ids=instrument_ids,
                as_nautilus=as_nautilus,
                **kwargs,
            )

    def _query(  # noqa (too complex)
        self,
        cls: type,
        instrument_ids: Optional[list[str]] = None,
        filter_expr: Optional[Callable] = None,
        start: Optional[Union[pd.Timestamp, str, int]] = None,
        end: Optional[Union[pd.Timestamp, str, int]] = None,
        ts_column: str = "ts_init",
        raise_on_empty: bool = True,
        instrument_id_column="instrument_id",
        table_kwargs: Optional[dict] = None,
        clean_instrument_keys: bool = True,
        as_dataframe: bool = True,
        projections: Optional[dict] = None,
        **kwargs,
    ):
        filters = [filter_expr] if filter_expr is not None else []
        if instrument_ids is not None:
            if not isinstance(instrument_ids, list):
                instrument_ids = [instrument_ids]
            if clean_instrument_keys:
                instrument_ids = list(set(map(clean_key, instrument_ids)))
            filters.append(ds.field(instrument_id_column).cast("string").isin(instrument_ids))
        if start is not None:
            filters.append(ds.field(ts_column) >= pd.Timestamp(start).value)
        if end is not None:
            filters.append(ds.field(ts_column) <= pd.Timestamp(end).value)

        full_path = self.make_path(cls=cls)

        if not (self.fs.exists(full_path) or self.fs.isdir(full_path)):
            if raise_on_empty:
                raise FileNotFoundError(f"protocol={self.fs.protocol}, path={full_path}")
            else:
                return pd.DataFrame() if as_dataframe else None

        # Load rust objects
        if isinstance(start, int) or start is None:
            start_nanos = start
        else:
            start_nanos = dt_to_unix_nanos(start)  # datetime > nanos

        if isinstance(end, int) or end is None:
            end_nanos = end
        else:
            end_nanos = dt_to_unix_nanos(end)  # datetime > nanos

        use_rust = kwargs.get("use_rust") and cls in (QuoteTick, TradeTick)
        if use_rust and kwargs.get("as_nautilus"):
            assert instrument_ids is not None
            assert len(instrument_ids) > 0

            to_merge = []
            for instrument_id in instrument_ids:
                files = self.get_files(cls, instrument_id, start_nanos, end_nanos)

                if raise_on_empty and not files:
                    raise RuntimeError("No files found.")

                batches = generate_batches_rust(
                    files=files,
                    cls=cls,
                    batch_size=sys.maxsize,
                    start_nanos=start_nanos,
                    end_nanos=end_nanos,
                )
                objs = list(itertools.chain.from_iterable(batches))
                if len(instrument_ids) == 1:
                    return objs  # skip merge, only 1 instrument
                to_merge.append(objs)

            return list(heapq.merge(*to_merge, key=lambda x: x.ts_init))

        dataset = ds.dataset(full_path, partitioning="hive", filesystem=self.fs)

        table_kwargs = table_kwargs or {}
        if projections:
            projected = {**{c: ds.field(c) for c in dataset.schema.names}, **projections}
            table_kwargs.update(columns=projected)

        try:
            table = dataset.to_table(filter=combine_filters(*filters), **(table_kwargs or {}))
        except Exception as e:
            print(e)
            raise e

        if use_rust:
            df = int_to_float_dataframe(table.to_pandas())
            if start_nanos and end_nanos is None:
                return df
            if start_nanos is None:
                start_nanos = 0
            if end_nanos is None:
                end_nanos = sys.maxsize
            df = df[(df["ts_init"] >= start_nanos) & (df["ts_init"] <= end_nanos)]
            return df

        mappings = self.load_inverse_mappings(path=full_path)

        if "as_nautilus" in kwargs:
            as_dataframe = not kwargs.pop("as_nautilus")

        if as_dataframe:
            return self._handle_table_dataframe(
                table=table, mappings=mappings, raise_on_empty=raise_on_empty, **kwargs
            )
        else:
            return self._handle_table_nautilus(table=table, cls=cls, mappings=mappings)

    def make_path(self, cls: type, instrument_id: Optional[str] = None) -> str:
        path = f"{self.path}/data/{class_to_filename(cls=cls)}.parquet"
        if instrument_id is not None:
            path += f"/instrument_id={clean_key(instrument_id)}"
        return path

    def get_files(
        self,
        cls: type,
        instrument_id: Optional[str] = None,
        start_nanos: Optional[int] = None,
        end_nanos: Optional[int] = None,
        bar_spec: Optional[BarSpecification] = None,
    ) -> list[str]:
        if instrument_id is None:
            folder = self.path
        else:
            folder = self.make_path(cls=cls, instrument_id=instrument_id)

        "/var/folders/fc/g4mqb35j0jvf7zpj4k76j4yw0000gn/T/tmp7cdq2cbx/data/order_book_data.parquet/instrument_id=1.166564490-237491-0.0.BETFAIR"
        "/var/folders/fc/g4mqb35j0jvf7zpj4k76j4yw0000gn/T/tmp7cdq2cbx/data/order_book_data.parquet/instrument_id=1.166564490-237491-0.0.BETFAIR"

        if not self.fs.isdir(folder):
            return []

        paths = self.fs.glob(f"{folder}/**")

        file_paths = []
        for path in paths:
            # Filter by BarType
            bar_spec_matched = False
            if cls is Bar:
                bar_spec_matched = bar_spec and str(bar_spec) in path
                if not bar_spec_matched:
                    continue

            # Filter by time range
            file_path = pathlib.PurePosixPath(path).name
            matched = is_filename_in_time_range(file_path, start_nanos, end_nanos)
            if matched:
                file_paths.append(str(path))

        file_paths = sorted(file_paths, key=lambda x: Path(x).stem)

        return file_paths

    def _get_files(
        self,
        cls: type,
        instrument_id: Optional[str] = None,
        start_nanos: Optional[int] = None,
        end_nanos: Optional[int] = None,
    ) -> list[str]:
        if instrument_id is None:
            folder = self.path
        else:
            folder = self.make_path(cls=cls, instrument_id=instrument_id)

        if not os.path.exists(folder):
            return []

        paths = self.fs.glob(f"{folder}/**")

        files = []
        for path in paths:
            fn = pathlib.PurePosixPath(path).name
            matched = is_filename_in_time_range(fn, start_nanos, end_nanos)
            if matched:
                files.append(str(path))

        files = sorted(files, key=lambda x: Path(x).stem)

        return files

    def load_inverse_mappings(self, path):
        mappings = load_mappings(fs=self.fs, path=path)
        for key in mappings:
            mappings[key] = {v: k for k, v in mappings[key].items()}
        return mappings

    @staticmethod
    def _handle_table_dataframe(
        table: pa.Table,
        mappings: Optional[dict],
        raise_on_empty: bool = True,
        sort_columns: Optional[list] = None,
        as_type: Optional[dict] = None,
    ):
        df = table.to_pandas().drop_duplicates()
        for col in mappings:
            df.loc[:, col] = df[col].map(mappings[col])

        if df.empty and raise_on_empty:
            raise ValueError("Data empty")
        if sort_columns:
            df = df.sort_values(sort_columns)
        if as_type:
            df = df.astype(as_type)
        return df

    @staticmethod
    def _handle_table_nautilus(
        table: Union[pa.Table, pd.DataFrame],
        cls: type,
        mappings: Optional[dict],
    ):
        if isinstance(table, pa.Table):
            dicts = dict_of_lists_to_list_of_dicts(table.to_pydict())
        elif isinstance(table, pd.DataFrame):
            dicts = table.to_dict("records")
        else:
            raise TypeError(
                f"`table` was {type(table)}, expected `pyarrow.Table` or `pandas.DataFrame`",
            )
        if not dicts:
            return []
        for key, maps in mappings.items():
            for d in dicts:
                if d[key] in maps:
                    d[key] = maps[d[key]]
        data = ParquetSerializer.deserialize(cls=cls, chunk=dicts)
        return data

    def _query_subclasses(
        self,
        base_cls: type,
        instrument_ids: Optional[list[str]] = None,
        filter_expr: Optional[Callable] = None,
        as_nautilus: bool = False,
        **kwargs,
    ):
        subclasses = [base_cls] + base_cls.__subclasses__()

        dfs = []
        for cls in subclasses:
            try:
                df = self.query(
                    cls=cls,
                    filter_expr=filter_expr,
                    instrument_ids=instrument_ids,
                    raise_on_empty=False,
                    as_nautilus=as_nautilus,
                    **kwargs,
                )
                dfs.append(df)
            except ArrowInvalid as e:
                # If we're using a `filter_expr` here, there's a good chance
                # this error is using a filter that is specific to one set of
                # instruments and not to others, so we ignore it (if not; raise).
                if filter_expr is not None:
                    continue
                else:
                    raise e

        if not as_nautilus:
            return pd.concat([df for df in dfs if df is not None])
        else:
            objects = [o for objs in [df for df in dfs if df is not None] for o in objs]
            return objects

    # ---  OVERLOADED BASE METHODS ------------------------------------------------
    def generic_data(
        self,
        cls: type,
        as_nautilus: bool = False,
        metadata: Optional[dict] = None,
        filter_expr: Optional[Callable] = None,
        **kwargs,
    ):
        data = self._query(
            cls=cls,
            filter_expr=filter_expr,
            as_dataframe=not as_nautilus,
            **kwargs,
        )
        if as_nautilus:
            if data is None:
                return []
            return [GenericData(data_type=DataType(cls, metadata=metadata), data=d) for d in data]
        return data

    def instruments(
        self,
        instrument_type: Optional[type] = None,
        instrument_ids: Optional[list[str]] = None,
        **kwargs,
    ):
        kwargs["clean_instrument_keys"] = False
        return super().instruments(
            instrument_type=instrument_type,
            instrument_ids=instrument_ids,
            **kwargs,
        )

    def list_data_types(self):
        glob_path = f"{self.path}/data/*.parquet"
        return [pathlib.Path(p).stem for p in self.fs.glob(glob_path)]

    def list_partitions(self, cls_type: type):
        assert isinstance(cls_type, type), "`cls_type` should be type, i.e. TradeTick"
        name = class_to_filename(cls_type)
        dataset = pq.ParquetDataset(
            f"{self.path}/data/{name}.parquet",
            filesystem=self.fs,
        )
        partitions = {}
        for level in dataset.partitions.levels:
            partitions[level.name] = level.keys
        return partitions

    def list_backtests(self) -> list[str]:
        glob_path = f"{self.path}/backtest/*.feather"
        return [p.stem for p in map(Path, self.fs.glob(glob_path))]

    def list_live_runs(self) -> list[str]:
        glob_path = f"{self.path}/live/*.feather"
        return [p.stem for p in map(Path, self.fs.glob(glob_path))]

    def read_live_run(self, live_run_id: str, **kwargs):
        return self._read_feather(kind="live", run_id=live_run_id, **kwargs)

    def read_backtest(self, backtest_run_id: str, **kwargs):
        return self._read_feather(kind="backtest", run_id=backtest_run_id, **kwargs)

    def _read_feather(self, kind: str, run_id: str, raise_on_failed_deserialize: bool = False):
        class_mapping: dict[str, type] = {class_to_filename(cls): cls for cls in list_schemas()}
        data = {}
        glob_path = f"{self.path}/{kind}/{run_id}.feather/*.feather"

        for path in [p for p in self.fs.glob(glob_path)]:
            cls_name = camel_to_snake_case(pathlib.Path(path).stem).replace("__", "_")
            df = read_feather_file(path=path, fs=self.fs)

            if df is None:
                print(f"No data for {cls_name}")
                continue
            # Apply post read fixes
            try:
                objs = self._handle_table_nautilus(
                    table=df,
                    cls=class_mapping[cls_name],
                    mappings={},
                )
                data[cls_name] = objs
            except Exception as e:
                if raise_on_failed_deserialize:
                    raise
                print(f"Failed to deserialize {cls_name}: {e}")
        return sorted(sum(data.values(), list()), key=lambda x: x.ts_init)


def read_feather_file(path: str, fs: fsspec.AbstractFileSystem = None):
    fs = fs or fsspec.filesystem("file")
    if not fs.exists(path):
        return
    try:
        with fs.open(path) as f:
            reader = pa.ipc.open_stream(f)
            return reader.read_pandas()
    except (pa.ArrowInvalid, FileNotFoundError):
        return


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


def int_to_float_dataframe(df: pd.DataFrame):
    cols = [
        col
        for col, dtype in dict(df.dtypes).items()
        if dtype == np.int64 or dtype == np.uint64 and (col != "ts_event" and col != "ts_init")
    ]
    df[cols] = df[cols] / FIXED_SCALAR
    return df
