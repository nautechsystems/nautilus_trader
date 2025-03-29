# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import itertools
import os
import platform
from collections import defaultdict
from collections.abc import Callable
from collections.abc import Generator
from itertools import groupby
from os import PathLike
from pathlib import Path
from typing import Any, NamedTuple, Union

import fsspec
import numpy as np
import pandas as pd
import pyarrow as pa
import pyarrow.dataset as pds
import pyarrow.parquet as pq
from fsspec.implementations.local import make_path_posix
from fsspec.implementations.memory import MemoryFileSystem
from fsspec.utils import infer_storage_options
from pyarrow import ArrowInvalid

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.datetime import time_object_to_dt
from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.core.message import Event
from nautilus_trader.core.nautilus_pyo3 import DataBackendSession
from nautilus_trader.core.nautilus_pyo3 import NautilusDataType
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_list
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.persistence.catalog.base import BaseDataCatalog
from nautilus_trader.persistence.catalog.types import CatalogWriteMode
from nautilus_trader.persistence.funcs import class_to_filename
from nautilus_trader.persistence.funcs import combine_filters
from nautilus_trader.persistence.funcs import urisafe_instrument_id
from nautilus_trader.serialization.arrow.serializer import ArrowSerializer
from nautilus_trader.serialization.arrow.serializer import list_schemas


TimestampLike = int | str | float

NautilusRustDataType = Union[  # noqa: UP007 (mypy does not like pipe operators)
    nautilus_pyo3.OrderBookDelta,
    nautilus_pyo3.OrderBookDepth10,
    nautilus_pyo3.QuoteTick,
    nautilus_pyo3.TradeTick,
    nautilus_pyo3.Bar,
]


class FeatherFile(NamedTuple):
    path: str
    class_name: str


_NAUTILUS_PATH = "NAUTILUS_PATH"
_DEFAULT_FS_PROTOCOL = "file"


class ParquetDataCatalog(BaseDataCatalog):
    """
    Provides a queryable data catalog persisted to files in Parquet (Arrow) format.

    Parameters
    ----------
    path : PathLike[str] | str
        The root path for this data catalog. Must exist and must be an absolute path.
    fs_protocol : str, default 'file'
        The filesystem protocol used by `fsspec` to handle file operations.
        This determines how the data catalog interacts with storage, be it local filesystem,
        cloud storage, or others. Common protocols include 'file' for local storage,
        's3' for Amazon S3, and 'gcs' for Google Cloud Storage. If not provided, it defaults to 'file',
        meaning the catalog operates on the local filesystem.
    fs_storage_options : dict, optional
        The fs storage options.
    min_rows_per_group : int, default 0
        The minimum number of rows per group. When the value is greater than 0,
        the dataset writer will batch incoming data and only write the row
        groups to the disk when sufficient rows have accumulated.
    max_rows_per_group : int, default 5000
        The maximum number of rows per group. If the value is greater than 0,
        then the dataset writer may split up large incoming batches into
        multiple row groups.  If this value is set, then min_rows_per_group
        should also be set. Otherwise it could end up with very small row
        groups.
    show_query_paths : bool, default False
        If globed query paths should be printed to stdout.

    Warnings
    --------
    The data catalog is not threadsafe. Using it in a multithreaded environment can lead to
    unexpected behavior.

    Notes
    -----
    For further details about `fsspec` and its filesystem protocols, see
    https://filesystem-spec.readthedocs.io/en/latest/.

    """

    def __init__(
        self,
        path: PathLike[str] | str,
        fs_protocol: str | None = _DEFAULT_FS_PROTOCOL,
        fs_storage_options: dict | None = None,
        dataset_kwargs: dict | None = None,
        min_rows_per_group: int = 0,
        max_rows_per_group: int = 5_000,
        show_query_paths: bool = False,
    ) -> None:
        self.fs_protocol: str = fs_protocol or _DEFAULT_FS_PROTOCOL

        if isinstance(self.fs_protocol, str) and self.fs_protocol.startswith("("):
            print(f"Unexpected `fs_protocol` format: {self.fs_protocol}, defaulting to 'file'")
            self.fs_protocol = "file"

        self.fs_storage_options = fs_storage_options or {}
        self.fs: fsspec.AbstractFileSystem = fsspec.filesystem(
            self.fs_protocol,
            **self.fs_storage_options,
        )
        self.serializer = ArrowSerializer()
        self.dataset_kwargs = dataset_kwargs or {}
        self.min_rows_per_group = min_rows_per_group
        self.max_rows_per_group = max_rows_per_group
        self.show_query_paths = show_query_paths

        if self.fs_protocol == "file":
            final_path = str(make_path_posix(str(path)))
        else:
            final_path = str(path)

        if (
            isinstance(self.fs, MemoryFileSystem)
            and platform.system() == "Windows"
            and not final_path.startswith("/")
        ):
            final_path = "/" + final_path

        self.path = str(final_path)

    @classmethod
    def from_env(cls) -> ParquetDataCatalog:
        """
        Create a data catalog instance by accessing the 'NAUTILUS_PATH' environment
        variable.

        Returns
        -------
        ParquetDataCatalog

        Raises
        ------
        OSError
            If the 'NAUTILUS_PATH' environment variable is not set.

        """
        if _NAUTILUS_PATH not in os.environ:
            raise OSError(f"'{_NAUTILUS_PATH}' environment variable is not set.")

        return cls.from_uri(os.environ[_NAUTILUS_PATH] + "/catalog")

    @classmethod
    def from_uri(cls, uri: str) -> ParquetDataCatalog:
        """
        Create a data catalog instance from the given `uri`.

        Parameters
        ----------
        uri : str
            The URI string for the backing path.

        Returns
        -------
        ParquetDataCatalog

        """
        if "://" not in uri:
            # Assume a local path
            uri = "file://" + uri

        parsed = infer_storage_options(uri)
        path = parsed.pop("path")
        protocol = parsed.pop("protocol")
        storage_options = parsed.copy()

        return cls(path=path, fs_protocol=protocol, fs_storage_options=storage_options)

    # -- WRITING ----------------------------------------------------------------------------------

    def write_data(
        self,
        data: list[Data | Event] | list[NautilusRustDataType],
        basename_template: str = "part-{i}",
        mode: CatalogWriteMode = CatalogWriteMode.OVERWRITE,
        **kwargs: Any,
    ) -> None:
        """
        Write the given `data` to the catalog.

        The function categorizes the data based on their class name and, when applicable, their
        associated instrument ID. It then delegates the actual writing process to the
        `write_chunk` method.

        Parameters
        ----------
        data : list[Data | Event]
            The data or event objects to be written to the catalog.
        basename_template : str, default 'part-{i}'
            A template string used to generate basenames of written data files.
            The token '{i}' will be replaced with an automatically incremented
            integer as files are partitioned.
            If not specified, it defaults to 'part-{i}' + the default extension '.parquet'.
        mode : CatalogWriteMode, default 'OVERWRITE'
            The mode to use when writing data and when not using using the "partitioning" option.
            Can be one of the following:
            - CatalogWriteMode.APPEND: Appends the data to the existing data.
            - CatalogWriteMode.PREPEND: Prepends the data to the existing data.
            - CatalogWriteMode.OVERWRITE: Overwrites the existing data.
            - CatalogWriteMode.NEWFILE: Appends the data to the existing data by creating a new file.
        kwargs : Any
            Additional keyword arguments to be passed to the `write_chunk` method.

        Warnings
        --------
        Any existing data which already exists under a filename will be overwritten.
        If a `basename_template` is not provided, then its very likely existing data for the data type and instrument ID will
        be overwritten. To prevent data loss, ensure that the `basename_template` (or the default naming scheme)
        generates unique filenames for different data sets.

        Notes
        -----
         - All data of the same type is expected to be monotonically increasing, or non-decreasing.
         - The data is sorted and grouped based on its class name and instrument ID (if applicable) before writing.
         - Instrument-specific data should have either an `instrument_id` attribute or be an instance of `Instrument`.
         - The `Bar` class is treated as a special case, being grouped based on its `bar_type` attribute.
         - The input data list must be non-empty, and all data items must be of the appropriate class type.

        Raises
        ------
        ValueError
            If data of the same type is not monotonically increasing (or non-decreasing) based on `ts_init`.

        """

        def key(obj: Any) -> tuple[str, str | None]:
            name = type(obj).__name__

            if isinstance(obj, CustomData):
                obj = obj.data
                name = type(obj).__name__
            if isinstance(obj, Instrument):
                return name, obj.id.value
            elif hasattr(obj, "bar_type"):
                return name, str(obj.bar_type)
            elif hasattr(obj, "instrument_id"):
                return name, obj.instrument_id.value

            return name, None

        def obj_to_type(obj: Data) -> type:
            return type(obj) if not isinstance(obj, CustomData) else obj.data.__class__

        name_to_cls = {cls.__name__: cls for cls in {obj_to_type(d) for d in data}}

        for (cls_name, instrument_id), single_type in groupby(sorted(data, key=key), key=key):
            chunk = list(single_type)
            self.write_chunk(
                data=chunk,
                data_cls=name_to_cls[cls_name],
                instrument_id=instrument_id,
                basename_template=basename_template,
                mode=mode,
                **kwargs,
            )

    def write_chunk(
        self,
        data: list[Data],
        data_cls: type[Data],
        instrument_id: str | None = None,
        basename_template: str = "part-{i}",
        mode: CatalogWriteMode = CatalogWriteMode.OVERWRITE,
        **kwargs: Any,
    ) -> None:
        if isinstance(data[0], CustomData):
            data = [d.data for d in data]

        table = self._objects_to_table(data, data_cls=data_cls)
        path = self._make_path(data_cls=data_cls, instrument_id=instrument_id)
        kw = dict(**self.dataset_kwargs, **kwargs)

        if "partitioning" not in kw:
            self._fast_write(
                table=table,
                path=path,
                fs=self.fs,
                basename_template=basename_template,
                mode=mode,
            )
        else:
            # Write parquet file
            pds.write_dataset(
                data=table,
                base_dir=path,
                basename_template=f"{basename_template}.parquet",
                format="parquet",
                filesystem=self.fs,
                min_rows_per_group=self.min_rows_per_group,
                max_rows_per_group=self.max_rows_per_group,
                **kw,
            )

    def _objects_to_table(self, data: list[Data], data_cls: type) -> pa.Table:
        PyCondition.not_empty(data, "data")
        PyCondition.list_type(data, data_cls, "data")
        sorted_data = sorted(data, key=lambda x: x.ts_init)

        # Check data is strictly non-decreasing prior to write
        for original, sorted_version in zip(data, sorted_data, strict=False):
            if original.ts_init != sorted_version.ts_init:
                raise ValueError(
                    "Data should be monotonically increasing (or non-decreasing) based on `ts_init`: "
                    f"found {original.ts_init} followed by {sorted_version.ts_init}. "
                    "Consider sorting your data with something like "
                    "`data.sort(key=lambda x: x.ts_init)` prior to writing to the catalog",
                )

        table_or_batch = self.serializer.serialize_batch(data, data_cls=data_cls)
        assert table_or_batch is not None

        if isinstance(table_or_batch, pa.RecordBatch):
            return pa.Table.from_batches([table_or_batch])
        else:
            return table_or_batch

    def _make_path(self, data_cls: type[Data], instrument_id: str | None = None) -> str:
        if instrument_id is not None:
            assert isinstance(instrument_id, str), "instrument_id must be a string"
            clean_instrument_id = urisafe_instrument_id(instrument_id)

            return f"{self.path}/data/{class_to_filename(data_cls)}/{clean_instrument_id}"
        else:
            return f"{self.path}/data/{class_to_filename(data_cls)}"

    def _fast_write(
        self,
        table: pa.Table,
        path: str,
        fs: fsspec.AbstractFileSystem,
        basename_template: str,
        mode: CatalogWriteMode = CatalogWriteMode.OVERWRITE,
    ) -> None:
        fs.mkdirs(path, exist_ok=True)
        name = basename_template.format(i=0)
        parquet_file = f"{path}/{name}.parquet"
        empty_file = parquet_file
        i = 0

        while Path(empty_file).exists():
            i += 1
            name = basename_template.format(i=i)
            empty_file = f"{path}/{name}.parquet"

        if i > 1 and mode != CatalogWriteMode.NEWFILE:
            print(
                "Warning, Only CatalogWriteMode::NEWFILE is allowed for a directory containing several parquet files. Aborting write_data.",
            )
            return
        elif mode == CatalogWriteMode.NEWFILE:
            parquet_file = empty_file

        # following solution from https://stackoverflow.com/a/70817689
        if (
            mode in [CatalogWriteMode.APPEND, CatalogWriteMode.PREPEND]
            and Path(parquet_file).exists()
        ):
            existing_table = pq.read_table(source=parquet_file, pre_buffer=False, memory_map=True)
            table = table.cast(existing_table.schema)

            if mode == CatalogWriteMode.APPEND:
                combined_table = pa.concat_tables([existing_table, table])
                pq.write_table(combined_table, where=parquet_file)
            elif mode == CatalogWriteMode.PREPEND:
                combined_table = pa.concat_tables([table, existing_table])
                pq.write_table(combined_table, where=parquet_file)
        else:
            pq.write_table(
                table,
                where=parquet_file,
                filesystem=fs,
                row_group_size=self.max_rows_per_group,
            )

    def consolidate_data(
        self,
        data_cls: type,
        instrument_id: str | None = None,
        bar_type: str | None = None,
        ts_column: str = "ts_init",
    ) -> None:
        """
        Consolidate several parquet files into a single file with data sorted in
        ascending chronological order.

        Parameters
        ----------
        data_cls : type
            The data class type to consolidate.
        instrument_id : str or None, default None
            The specific instrument ID to consolidate.
        bar_type : str or None, default None
            The specific bar type to consolidate.
        ts_column : str, default "ts_init"
            The timestamp column name to use for sorting data.

        Notes
        -----
        The consolidation process combines multiple parquet files into a single file,
        with the data sorted chronologically based on the specified timestamp column.

        """
        parquet_files = self._query_parquet_files(data_cls, instrument_id, bar_type)

        if parquet_files is not None:
            _combine_data_files(parquet_files, ts_column)

    def consolidate_catalog(self, ts_column: str = "ts_init") -> None:
        """
        Consolidate all market data directories of the catalog containing several
        parquet files into a single file per directory, with data sorted in ascending
        chronological order.

        Parameters
        ----------
        ts_column : str, default "ts_init"
            The timestamp column name to use for sorting data.

        Notes
        -----
        The consolidation process combines multiple parquet files into a single file per directory,
        with the data sorted chronologically based on the specified timestamp column.

        """
        leaf_directories = self._find_leaf_data_directories()

        for directory in leaf_directories:
            parquet_files = self.fs.glob(os.path.join(directory, "*.parquet"))
            _combine_data_files(parquet_files, ts_column)

    def _find_leaf_data_directories(self) -> list[str]:
        all_paths = self.fs.glob(os.path.join(self.path, "data", "**"))
        all_dirs = [d for d in all_paths if self.fs.isdir(d)]
        leaf_dirs = []

        for directory in all_dirs:
            items = self.fs.glob(os.path.join(directory, "*"))
            has_subdirs = any(self.fs.isdir(item) for item in items)
            has_files = any(self.fs.isfile(item) for item in items)

            if has_files and not has_subdirs:
                leaf_dirs.append(directory)

        return leaf_dirs

    # -- QUERIES ----------------------------------------------------------------------------------

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
                    data_cls=cls,
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

    def query(
        self,
        data_cls: type,
        instrument_ids: list[str] | None = None,
        bar_types: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        where: str | None = None,
        **kwargs: Any,
    ) -> list[Data | CustomData]:
        if self.fs_protocol == "file" and data_cls in (
            OrderBookDelta,
            OrderBookDeltas,
            OrderBookDepth10,
            QuoteTick,
            TradeTick,
            Bar,
        ):
            data = self.query_rust(
                data_cls=data_cls,
                instrument_ids=instrument_ids,
                bar_types=bar_types,
                start=start,
                end=end,
                where=where,
                **kwargs,
            )
        else:
            data = self.query_pyarrow(
                data_cls=data_cls,
                instrument_ids=instrument_ids,
                bar_types=bar_types,
                start=start,
                end=end,
                where=where,
                **kwargs,
            )

        if not is_nautilus_class(data_cls):
            # Special handling for generic data
            metadata = kwargs.get("metadata")
            data = [
                CustomData(data_type=DataType(data_cls, metadata=metadata), data=d) for d in data
            ]

        return data

    def query_rust(
        self,
        data_cls: type,
        instrument_ids: list[str] | None = None,
        bar_types: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        where: str | None = None,
        **kwargs: Any,
    ) -> list[Data]:
        query_data_cls = OrderBookDelta if data_cls == OrderBookDeltas else data_cls
        session = self.backend_session(
            data_cls=query_data_cls,
            instrument_ids=instrument_ids,
            bar_types=bar_types,
            start=start,
            end=end,
            where=where,
            **kwargs,
        )

        result = session.to_query_result()

        # Gather data
        data = []
        for chunk in result:
            data.extend(capsule_to_list(chunk))

        if data_cls == OrderBookDeltas:
            # Batch process deltas into `OrderBookDeltas`, will warn
            # when there are deltas after the final `F_LAST` flag.
            data = OrderBookDeltas.batch(data)

        return data

    def backend_session(
        self,
        data_cls: type,
        instrument_ids: list[str] | None = None,
        bar_types: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        where: str | None = None,
        session: DataBackendSession | None = None,
        **kwargs: Any,
    ) -> DataBackendSession:
        assert self.fs_protocol == "file", "Only file:// protocol is supported for Rust queries"
        data_type: NautilusDataType = ParquetDataCatalog._nautilus_data_cls_to_data_type(data_cls)

        if session is None:
            session = DataBackendSession()

        file_prefix = class_to_filename(data_cls)
        glob_path = f"{self.path}/data/{file_prefix}/**/*"
        paths: list[str] = self.fs.glob(glob_path)

        # Ensure all paths are files (fsspec now includes directories in recursive globbing)
        paths = [path for path in paths if self.fs.isfile(path)]

        if self.show_query_paths:
            for dir in paths:
                print(dir)

        for idx, path in enumerate(paths):
            assert self.fs.exists(path)
            # Parse the parent directory which *should* be the instrument ID,
            # this prevents us matching all instrument ID substrings.
            dir = path.split("/")[-2]

            # Filter by instrument ID
            if data_cls == Bar:
                if instrument_ids and not any(
                    dir.startswith(urisafe_instrument_id(x) + "-") for x in instrument_ids
                ):
                    continue
            elif instrument_ids and not any(
                dir == urisafe_instrument_id(x) for x in instrument_ids
            ):
                continue

            # Filter by bar type
            if bar_types and not any(dir == urisafe_instrument_id(x) for x in bar_types):
                continue

            table = f"{file_prefix}_{idx}"
            query = self._build_query(
                table,
                # instrument_ids=None, # Filtering by filename for now
                start=start,
                end=end,
                where=where,
            )
            session.add_file(data_type, table, str(path), query)

        return session

    @staticmethod
    def _nautilus_data_cls_to_data_type(data_cls: type) -> NautilusDataType:
        if data_cls in (OrderBookDelta, OrderBookDeltas):
            return NautilusDataType.OrderBookDelta
        elif data_cls == OrderBookDepth10:
            return NautilusDataType.OrderBookDepth10
        elif data_cls == QuoteTick:
            return NautilusDataType.QuoteTick
        elif data_cls == TradeTick:
            return NautilusDataType.TradeTick
        elif data_cls == Bar:
            return NautilusDataType.Bar
        else:
            raise RuntimeError(f"unsupported `data_cls` for Rust parquet, was {data_cls.__name__}")

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

    def query_pyarrow(
        self,
        data_cls: type,
        instrument_ids: list[str] | None = None,
        bar_types: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        filter_expr: str | None = None,
        **kwargs: Any,
    ) -> list[Data]:
        table = self._load_pyarrow_table(
            data_cls=data_cls,
            filter_expr=filter_expr,
            instrument_ids=instrument_ids,
            bar_types=bar_types,
            start=start,
            end=end,
        )

        if table is None:
            return []

        assert (
            table.num_rows
        ), f"No rows found for {data_cls=} {instrument_ids=} {filter_expr=} {start=} {end=}"

        return self._handle_table_nautilus(table, data_cls=data_cls)

    def _load_pyarrow_table(
        self,
        data_cls: type,
        filter_expr: str | None = None,
        instrument_ids: list[str] | None = None,
        bar_types: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        ts_column: str = "ts_init",
    ) -> pds.Dataset | None:
        dataset = self._load_dataset(
            data_cls=data_cls,
            instrument_ids=instrument_ids,
            bar_types=bar_types,
        )

        if dataset is None:
            return None

        return self._filter_dataset(
            dataset=dataset,
            filter_expr=filter_expr,
            start=start,
            end=end,
            ts_column=ts_column,
        )

    def _load_dataset(
        self,
        data_cls: type,
        instrument_ids: list[str] | str | None = None,
        bar_types: list[str] | str | None = None,
    ) -> pds.Dataset | list[str] | None:
        file_prefix = class_to_filename(data_cls)
        dataset_path = f"{self.path}/data/{file_prefix}"

        if not self.fs.exists(dataset_path):
            return None

        # Original dataset
        dataset = pds.dataset(dataset_path, filesystem=self.fs)

        # Instrument id filters (not stored in table, need to filter based on files)
        if instrument_ids is not None:
            if not isinstance(instrument_ids, list):
                instrument_ids = [instrument_ids]

            valid_files = [
                fn
                for fn in dataset.files
                if any(urisafe_instrument_id(x) in fn for x in instrument_ids)
            ]

            dataset = pds.dataset(valid_files, filesystem=self.fs)

        if bar_types is not None:
            if not isinstance(bar_types, list):
                bar_types = [bar_types]

            valid_files = [
                fn for fn in dataset.files if any(str(x).replace("/", "") in fn for x in bar_types)
            ]

            dataset = pds.dataset(valid_files, filesystem=self.fs)

        if dataset is None:
            return None

        return dataset

    def _filter_dataset(
        self,
        dataset: pds.Dataset,
        filter_expr: str | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        ts_column: str = "ts_init",
    ) -> pds.Dataset | None:
        if dataset is None:
            return None

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

    @staticmethod
    def _handle_table_nautilus(
        table: pa.Table | pd.DataFrame,
        data_cls: type,
    ) -> list[Data]:
        if isinstance(table, pd.DataFrame):
            table = pa.Table.from_pandas(table)

        data = ArrowSerializer.deserialize(data_cls=data_cls, batch=table)
        # TODO (bm/cs) remove when pyo3 objects are used everywhere.
        module = data[0].__class__.__module__

        if "nautilus_pyo3" in module:
            cython_cls = {
                "OrderBookDelta": OrderBookDelta,
                "OrderBookDeltas": OrderBookDelta,
                "OrderBookDepth10": OrderBookDepth10,
                "QuoteTick": QuoteTick,
                "TradeTick": TradeTick,
                "Bar": Bar,
            }.get(data_cls.__name__, data_cls.__name__)
            data = cython_cls.from_pyo3_list(data)

        return data

    def query_timestamp_bound(
        self,
        data_cls: type,
        instrument_id: str | None = None,
        bar_type: str | None = None,
        ts_column: str = "ts_init",
        is_last: bool = True,
    ) -> pd.Timestamp | None:
        if data_cls == Instrument:
            for instrument_type in Instrument.__subclasses__():
                last_timestamp = self._query_timestamp_bound(
                    data_cls=instrument_type,
                    instrument_id=instrument_id,
                    bar_type=bar_type,
                    ts_column=ts_column,
                    is_last=is_last,
                )

                if last_timestamp is not None:
                    return last_timestamp

            return None

        return self._query_timestamp_bound(
            data_cls=data_cls,
            instrument_id=instrument_id,
            bar_type=bar_type,
            ts_column=ts_column,
            is_last=is_last,
        )

    def _query_timestamp_bound(
        self,
        data_cls: type,
        instrument_id: str | None = None,
        bar_type: str | None = None,
        ts_column: str = "ts_init",
        is_last: bool = True,
    ) -> pd.Timestamp | None:
        parquet_files = self._query_parquet_files(data_cls, instrument_id, bar_type)

        if parquet_files is None or len(parquet_files) == 0:
            return None

        min_max_per_file = np.array(
            [_min_max_from_parquet_metadata(file, ts_column) for file in parquet_files],
        )

        if is_last:
            return time_object_to_dt(min_max_per_file[:, 1].max())
        else:
            return time_object_to_dt(min_max_per_file[:, 0].min())

    def _query_parquet_files(
        self,
        data_cls: type,
        instrument_id: str | None = None,
        bar_type: str | None = None,
    ) -> list[str] | None:
        file_prefix = class_to_filename(data_cls)
        directory = f"{self.path}/data/{file_prefix}"

        if instrument_id is not None:
            directory += f"/{urisafe_instrument_id(instrument_id)}"

        if data_cls is Bar:
            if bar_type is None:
                print("A bar_type should be specified for querying Bar parquet files. Aborting.")
                return None

            bar_type_dir = str(bar_type).replace("/", "")
            directory += f"/{bar_type_dir}"

        return self.fs.glob(os.path.join(directory, "*.parquet"))

    # -- OVERLOADED BASE METHODS ------------------------------------------------------------------

    def _list_directory_stems(self, subdirectory: str) -> list[str]:
        glob_path = f"{self.path}/{subdirectory}/*"
        return [Path(p).stem for p in self.fs.glob(glob_path)]

    def list_data_types(self) -> list[str]:
        return self._list_directory_stems("data")

    def list_backtest_runs(self) -> list[str]:
        return self._list_directory_stems("backtest")

    def list_live_runs(self) -> list[str]:
        return self._list_directory_stems("live")

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
        class_mapping: dict[str, type] = {class_to_filename(cls): cls for cls in list_schemas()}
        data = defaultdict(list)

        for feather_file in self._list_feather_files(kind=kind, instance_id=instance_id):
            path = feather_file.path
            cls_name = feather_file.class_name
            table: pa.Table = self._read_feather_file(path=path)

            if table is None or len(table) == 0:
                continue

            if table is None:
                print(f"No data for {cls_name}")
                continue
            # Apply post read fixes
            try:
                data_cls = class_mapping[cls_name]
                objs = self._handle_table_nautilus(table=table, data_cls=data_cls)
                data[cls_name].extend(objs)
            except Exception as e:
                if raise_on_failed_deserialize:
                    raise

                print(f"Failed to deserialize {cls_name}: {e}")

        return sorted(itertools.chain.from_iterable(data.values()), key=lambda x: x.ts_init)

    def _list_feather_files(
        self,
        kind: str,
        instance_id: str,
    ) -> Generator[FeatherFile, None, None]:
        prefix = f"{self.path}/{kind}/{urisafe_instrument_id(instance_id)}"

        # Non-instrument feather files
        for path_str in self.fs.glob(f"{prefix}/*.feather"):
            if not Path(path_str).is_file():
                continue

            file_name = path_str.replace(prefix + "/", "").replace(".feather", "")
            cls_name = "_".join(file_name.split("_")[:-1])

            if not cls_name:
                raise ValueError(f"`cls_name` was empty when a value was expected: {path_str}")

            yield FeatherFile(path=path_str, class_name=cls_name)

        # Per-instrument feather files
        for path_str in self.fs.glob(f"{prefix}/**/*.feather"):
            if not Path(path_str).is_file():
                continue

            file_name = path_str.replace(prefix + "/", "").replace(".feather", "")
            cls_name = Path(file_name).parent.name

            if not cls_name:
                continue

            yield FeatherFile(path=path_str, class_name=cls_name)

    def _read_feather_file(
        self,
        path: str,
    ) -> pa.Table | None:
        if not self.fs.exists(path):
            return None
        try:
            with self.fs.open(path) as f:
                reader = pa.ipc.open_stream(f)
                return reader.read_all()
        except (pa.ArrowInvalid, OSError):
            return None

    def convert_stream_to_data(
        self,
        instance_id: str,
        data_cls: type,
        other_catalog: ParquetDataCatalog | None = None,
        subdirectory: str = "backtest",
        **kwargs: Any,
    ) -> None:
        table_name = class_to_filename(data_cls)
        feather_dir = Path(self.path) / subdirectory / instance_id

        if (feather_dir / table_name).is_dir():
            feather_files = sorted((feather_dir / table_name).glob("*.feather"))
        else:
            feather_files = sorted(feather_dir.glob(f"{table_name}_*.feather"))

        all_data = []
        for feather_file in feather_files:
            feather_table = self._read_feather_file(str(feather_file))

            if feather_table is not None:
                custom_data_list = self._handle_table_nautilus(feather_table, data_cls)
                all_data.extend(custom_data_list)

        all_data.sort(key=lambda x: x.ts_init)
        used_catalog = self if other_catalog is None else other_catalog
        used_catalog.write_data(all_data, **kwargs)


def _min_max_from_parquet_metadata(file_path: str, column_name: str) -> tuple[int, int]:
    parquet_file = pq.ParquetFile(file_path)
    metadata = parquet_file.metadata

    overall_min_value = None
    overall_max_value = None

    for i in range(metadata.num_row_groups):
        row_group_metadata = metadata.row_group(i)

        for j in range(row_group_metadata.num_columns):
            col_metadata = row_group_metadata.column(j)

            if col_metadata.path_in_schema == column_name:
                if col_metadata.statistics is not None:
                    min_value = col_metadata.statistics.min
                    max_value = col_metadata.statistics.max

                    if overall_min_value is None or min_value < overall_min_value:
                        overall_min_value = min_value
                    if overall_max_value is None or max_value > overall_max_value:
                        overall_max_value = max_value
                else:
                    print(
                        f"Warning: Statistics not available for column '{column_name}' in row group {i}.",
                    )

    if overall_min_value is None or overall_max_value is None:
        print(f"Column '{column_name}' not found or has no statistics in any row group.")
        return -1, -1
    else:
        return overall_min_value, overall_max_value


def _combine_parquet_files(file_list: list[str]) -> None:
    if len(file_list) <= 1:
        return

    tables = [pq.read_table(file, memory_map=True, pre_buffer=False) for file in file_list]
    combined_table = pa.concat_tables(tables)
    pq.write_table(combined_table, where=file_list[0])

    for file_path in file_list[1:]:
        os.remove(file_path)


def _combine_data_files(parquet_files, ts_column):
    n_files = len(parquet_files)

    if n_files <= 1:
        return

    # ordering by first timestamp of each file
    min_max_per_file = [_min_max_from_parquet_metadata(file, ts_column) for file in parquet_files]
    ordering = sorted(range(n_files), key=lambda i: min_max_per_file[i][0])

    for i in range(1, n_files):
        # last timestamp of previous sorted file bigger than first time timestamp of current file
        if min_max_per_file[ordering[i - 1]][1] >= min_max_per_file[ordering[i]][0]:
            print("Merging not safe due to intersection of timestamps between files. Aborting.")
            return

    sorted_parquet_files = [parquet_files[i] for i in ordering]
    _combine_parquet_files(sorted_parquet_files)
