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

from __future__ import annotations

import os
import pathlib
import platform
from collections import defaultdict
from collections.abc import Generator
from itertools import groupby
from pathlib import Path
from typing import Any, Callable, NamedTuple, Union

import fsspec
import pandas as pd
import pyarrow as pa
import pyarrow.dataset as pds
import pyarrow.parquet as pq
from fsspec.implementations.local import make_path_posix
from fsspec.implementations.memory import MemoryFileSystem
from fsspec.utils import infer_storage_options
from pyarrow import ArrowInvalid

from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.core.message import Event
from nautilus_trader.core.nautilus_pyo3.persistence import DataBackendSession
from nautilus_trader.core.nautilus_pyo3.persistence import NautilusDataType
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import GenericData
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data.book import OrderBookDelta
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.persistence.catalog.base import BaseDataCatalog
from nautilus_trader.persistence.catalog.parquet.util import class_to_filename
from nautilus_trader.persistence.catalog.parquet.util import combine_filters
from nautilus_trader.persistence.catalog.parquet.util import uri_instrument_id
from nautilus_trader.persistence.wranglers import list_from_capsule
from nautilus_trader.serialization.arrow.serializer import ArrowSerializer
from nautilus_trader.serialization.arrow.serializer import list_schemas


TimestampLike = Union[int, str, float]


class FeatherFile(NamedTuple):
    path: str
    class_name: str


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
        fs_protocol: str = "file",
        fs_storage_options: dict | None = None,
        dataset_kwargs: dict | None = None,
    ):
        self.fs_protocol = fs_protocol
        self.fs_storage_options = fs_storage_options or {}
        self.fs: fsspec.AbstractFileSystem = fsspec.filesystem(
            self.fs_protocol,
            **self.fs_storage_options,
        )
        self.serializer = ArrowSerializer()
        self.dataset_kwargs = dataset_kwargs or {}

        path = make_path_posix(str(path))

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

    # -- WRITING -----------------------------------------------------------------------------------
    def _objects_to_table(self, data: list[Data], cls: type) -> pa.Table:
        assert len(data) > 0
        assert all(type(obj) is cls for obj in data)  # same type
        table = self.serializer.serialize_batch(data, cls=cls)
        assert table is not None
        if isinstance(table, pa.RecordBatch):
            table = pa.Table.from_batches([table])
        return table

    def _make_path(self, cls: type[Data], instrument_id: str | None = None) -> str:
        if instrument_id is not None:
            assert isinstance(instrument_id, str), "instrument_id must be a string"
            clean_instrument_id = uri_instrument_id(instrument_id)
            return f"{self.path}/data/{class_to_filename(cls)}/{clean_instrument_id}"
        else:
            return f"{self.path}/data/{class_to_filename(cls)}"

    def write_chunk(
        self,
        data: list[Data],
        cls: type[Data],
        instrument_id: str | None = None,
        **kwargs: Any,
    ) -> None:
        table = self._objects_to_table(data, cls=cls)
        path = self._make_path(cls=cls, instrument_id=instrument_id)
        kw = dict(**self.dataset_kwargs, **kwargs)

        if "partitioning" not in kw:
            self._fast_write(table=table, path=path, fs=self.fs)
        else:
            # Write parquet file
            pds.write_dataset(
                data=table,
                base_dir=path,
                format="parquet",
                filesystem=self.fs,
                **self.dataset_kwargs,
                **kwargs,
            )

    def _fast_write(
        self,
        table: pa.Table,
        path: str,
        fs: fsspec.AbstractFileSystem,
    ) -> None:
        fs.mkdirs(path, exist_ok=True)
        pq.write_table(table, where=f"{path}/part-0.parquet", filesystem=fs)

    def write_data(self, data: list[Data | Event], **kwargs: Any) -> None:
        def key(obj: Any) -> tuple[str, str | None]:
            name = type(obj).__name__
            if isinstance(obj, Instrument):
                return name, obj.id.value
            elif isinstance(obj, Bar):
                return name, str(obj.bar_type)
            elif hasattr(obj, "instrument_id"):
                return name, obj.instrument_id.value
            return name, None

        name_to_cls = {cls.__name__: cls for cls in {type(d) for d in data}}
        for (cls_name, instrument_id), single_type in groupby(sorted(data, key=key), key=key):
            self.write_chunk(
                data=list(single_type),
                cls=name_to_cls[cls_name],
                instrument_id=instrument_id,
                **kwargs,
            )

    # -- QUERIES ----------------------------------------------------------------------------------

    def query(
        self,
        cls: type,
        instrument_ids: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        where: str | None = None,
        **kwargs: Any,
    ) -> list[Data | GenericData]:
        if cls in (QuoteTick, TradeTick, Bar, OrderBookDelta):
            data = self.query_rust(
                cls=cls,
                instrument_ids=instrument_ids,
                start=start,
                end=end,
                where=where,
                **kwargs,
            )
        else:
            data = self.query_pyarrow(
                cls=cls,
                instrument_ids=instrument_ids,
                start=start,
                end=end,
                where=where,
                **kwargs,
            )

        if not is_nautilus_class(cls):
            # Special handling for generic data
            data = [
                GenericData(data_type=DataType(cls, metadata=kwargs.get("metadata")), data=d)
                for d in data
            ]
        return data

    def query_rust(
        self,
        cls: type,
        instrument_ids: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        where: str | None = None,
        **kwargs: Any,
    ) -> list[Data]:
        assert self.fs_protocol == "file", "Only file:// protocol is supported for Rust queries"
        name = cls.__name__
        file_prefix = class_to_filename(cls)
        data_type = getattr(NautilusDataType, {"OrderBookDeltas": "OrderBookDelta"}.get(name, name))

        session = DataBackendSession()
        # TODO (bm) - fix this glob, query once on catalog creation?
        glob_path = f"{self.path}/data/{file_prefix}/**/*"
        dirs = self.fs.glob(glob_path)
        for idx, fn in enumerate(dirs):
            assert self.fs.exists(fn)
            if instrument_ids and not any(uri_instrument_id(id_) in fn for id_ in instrument_ids):
                continue
            table = f"{file_prefix}_{idx}"
            query = self._build_query(
                table,
                # instrument_ids=None, # Filtering by filename for now.
                start=start,
                end=end,
                where=where,
            )

            session.add_file_with_query(table, fn, query, data_type)

        result = session.to_query_result()

        # Gather data
        data = []
        for chunk in result:
            data.extend(list_from_capsule(chunk))

        return data

    def query_pyarrow(
        self,
        cls: type,
        instrument_ids: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        filter_expr: str | None = None,
        **kwargs: Any,
    ) -> list[Data]:
        file_prefix = class_to_filename(cls)
        dataset_path = f"{self.path}/data/{file_prefix}"
        if not self.fs.exists(dataset_path):
            return []
        table = self._load_pyarrow_table(
            path=dataset_path,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            start=start,
            end=end,
        )

        assert (
            table is not None
        ), f"No table found for {cls=} {instrument_ids=} {filter_expr=} {start=} {end=}"
        assert (
            table.num_rows
        ), f"No rows found for {cls=} {instrument_ids=} {filter_expr=} {start=} {end=}"

        return self._handle_table_nautilus(table, cls=cls)

    def _load_pyarrow_table(
        self,
        path: str,
        filter_expr: str | None = None,
        instrument_ids: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        ts_column: str = "ts_init",
    ) -> pds.Dataset | None:
        # Original dataset
        dataset = pds.dataset(path, filesystem=self.fs)

        # Instrument id filters (not stored in table, need to filter based on files)
        if instrument_ids is not None:
            if not isinstance(instrument_ids, list):
                instrument_ids = [instrument_ids]
            valid_files = [
                fn
                for fn in dataset.files
                if any(uri_instrument_id(x) in fn for x in instrument_ids)
            ]
            dataset = pds.dataset(valid_files, filesystem=self.fs)

        filters: list[pds.Expression] = [filter_expr] if filter_expr is not None else []
        if start is not None:
            filters.append(pds.field(ts_column) >= pd.Timestamp(start).value)
        if end is not None:
            filters.append(pds.field(ts_column) <= pd.Timestamp(end).value)
        if filters:
            filter_ = combine_filters(*filters)
        else:
            filter_ = None
        return dataset.to_table(filter=filter_)

    def _build_query(
        self,
        table: str,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        where: str | None = None,
    ) -> str:
        # Build datafusion SQL query
        query = f"SELECT * FROM {table}"  # noqa (possible SQL injection)
        conditions: list[str] = [] + ([where] if where else [])
        # if len(instrument_ids or []) == 1:
        #     conditions.append(f"instrument_id = '{instrument_ids[0]}'")
        # elif instrument_ids:
        #     conditions.append(f"instrument_id in {tuple(instrument_ids)}")
        if start:
            start_ts = dt_to_unix_nanos(start)
            conditions.append(f"ts_init >= {start_ts}")
        if end:
            end_ts = dt_to_unix_nanos(end)
            conditions.append(f"ts_init <= {end_ts}")
        if conditions:
            query += f" WHERE {' AND '.join(conditions)}"
        query += " ORDER BY ts_init"
        return query

    @staticmethod
    def _handle_table_nautilus(
        table: pa.Table | pd.DataFrame,
        cls: type,
    ) -> list[Data]:
        if isinstance(table, pd.DataFrame):
            table = pa.Table.from_pandas(table)
        data = ArrowSerializer.deserialize(cls=cls, batch=table)
        # TODO (bm/cs) remove when pyo3 objects are used everywhere.
        module = data[0].__class__.__module__
        if "builtins" in module:
            cython_cls = {
                "OrderBookDeltas": OrderBookDelta,
                "OrderBookDelta": OrderBookDelta,
                "TradeTick": TradeTick,
                "QuoteTick": QuoteTick,
                "Bar": Bar,
            }.get(cls.__name__, cls.__name__)
            data = cython_cls.from_pyo3(data)
        return data

    def _query_subclasses(
        self,
        base_cls: type,
        instrument_ids: list[str] | None = None,
        filter_expr: Callable | None = None,
        **kwargs: Any,
    ) -> list[Data]:
        subclasses = [base_cls, *base_cls.__subclasses__()]

        dfs = []
        for cls in subclasses:
            try:
                df = self.query(
                    cls=cls,
                    filter_expr=filter_expr,
                    instrument_ids=instrument_ids,
                    raise_on_empty=False,
                    **kwargs,
                )
                dfs.append(df)
            except AssertionError as e:
                if "No rows found for" in str(e):
                    continue
                raise
            except ArrowInvalid as e:
                # If we're using a `filter_expr` here, there's a good chance
                # this error is using a filter that is specific to one set of
                # instruments and not to others, so we ignore it (if not; raise).
                if filter_expr is not None:
                    continue
                else:
                    raise e

        objects = [o for objs in [df for df in dfs if df is not None] for o in objs]
        return objects

    # -- OVERLOADED BASE METHODS ------------------------------------------------------------------

    def instruments(
        self,
        instrument_type: type | None = None,
        instrument_ids: list[str] | None = None,
        **kwargs: Any,
    ) -> list[Instrument]:
        return super().instruments(
            instrument_type=instrument_type,
            instrument_ids=instrument_ids,
            **kwargs,
        )

    def list_data_types(self):
        glob_path = f"{self.path}/data/*"
        return [pathlib.Path(p).stem for p in self.fs.glob(glob_path)]

    def list_backtest_runs(self) -> list[str]:
        glob_path = f"{self.path}/backtest/*"
        return [p.stem for p in map(Path, self.fs.glob(glob_path))]

    def list_live_runs(self) -> list[str]:
        glob_path = f"{self.path}/live/*"
        return [p.stem for p in map(Path, self.fs.glob(glob_path))]

    def read_live_run(self, instance_id: str, **kwargs: Any) -> list[Data]:
        return self._read_feather(kind="live", instance_id=instance_id, **kwargs)

    def read_backtest(self, instance_id: str, **kwargs: Any) -> list[Data]:
        return self._read_feather(kind="backtest", instance_id=instance_id, **kwargs)

    def _read_feather(
        self,
        kind: str,
        instance_id: str,
        raise_on_failed_deserialize: bool = False,
    ) -> list[Data]:
        from nautilus_trader.persistence.streaming.writer import read_feather_file

        class_mapping: dict[str, type] = {class_to_filename(cls): cls for cls in list_schemas()}
        data = defaultdict(list)
        for feather_file in self._list_feather_files(kind=kind, instance_id=instance_id):
            path = feather_file.path
            cls_name = feather_file.class_name
            table: pa.Table = read_feather_file(path=path, fs=self.fs)
            if table is None or len(table) == 0:
                continue

            if table is None:
                print(f"No data for {cls_name}")
                continue
            # Apply post read fixes
            try:
                cls = class_mapping[cls_name]
                objs = self._handle_table_nautilus(table=table, cls=cls)
                data[cls_name].extend(objs)
            except Exception as e:
                if raise_on_failed_deserialize:
                    raise
                print(f"Failed to deserialize {cls_name}: {e}")
        return sorted(sum(data.values(), []), key=lambda x: x.ts_init)

    def _list_feather_files(
        self,
        kind: str,
        instance_id: str,
    ) -> Generator[FeatherFile, None, None]:
        prefix = f"{self.path}/{kind}/{uri_instrument_id(instance_id)}"

        # Non-instrument feather files
        for fn in self.fs.glob(f"{prefix}/*.feather"):
            cls_name = fn.replace(prefix + "/", "").replace(".feather", "")
            yield FeatherFile(path=fn, class_name=cls_name)

        # Per-instrument feather files
        for ins_fn in self.fs.glob(f"{prefix}/**/*.feather"):
            ins_cls_name = pathlib.Path(ins_fn.replace(prefix + "/", "")).parent.name
            yield FeatherFile(path=ins_fn, class_name=ins_cls_name)
