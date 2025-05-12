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
import re
from collections import defaultdict
from collections.abc import Callable
from collections.abc import Generator
from itertools import groupby
from os import PathLike
from pathlib import Path
from typing import Any, NamedTuple, Union

import fsspec
import pandas as pd
import portion as P
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
from nautilus_trader.model.data import MarkPriceUpdate
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import capsule_to_list
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.persistence.catalog.base import BaseDataCatalog
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
        start: int | None = None,
        end: int | None = None,
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
        start : int, optional
            The start timestamp for the data chunk.
        end : int, optional
            The end timestamp for the data chunk.

        Warnings
        --------
        Any existing data which already exists under a filename will be overwritten.

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
            self._write_chunk(
                data=chunk,
                data_cls=name_to_cls[cls_name],
                instrument_id=instrument_id,
            )

    def _write_chunk(
        self,
        data: list[Data],
        data_cls: type[Data],
        instrument_id: str | None = None,
        start: int | None = None,
        end: int | None = None,
    ) -> None:
        if isinstance(data[0], CustomData):
            data = [d.data for d in data]

        table = self._objects_to_table(data, data_cls=data_cls)
        directory = self._make_path(data_cls=data_cls, instrument_id=instrument_id)
        self.fs.mkdirs(directory, exist_ok=True)

        if isinstance(data[0], Instrument):
            # When writing an instrument for a given instrument_id, we don't want duplicates
            # Also keeping the first occurrence can give information about when it's first available
            data = [data[0]]

            for file in self.fs.glob(f"{directory}/*.parquet"):
                self.fs.rm(file)

        start = start if start else data[0].ts_init
        end = end if end else data[-1].ts_init
        parquet_file = f"{directory}/{start}-{end}.parquet"
        pq.write_table(
            table,
            where=parquet_file,
            filesystem=self.fs,
            row_group_size=self.max_rows_per_group,
        )

        intervals = self._get_directory_intervals(directory)
        assert _are_intervals_disjoint(
            intervals,
        ), "Intervals are not disjoint after writing a new file"

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

    def extend_file_name(
        self,
        data_cls: type[Data],
        instrument_id: str | None = None,
        start: int | None = None,
        end: int | None = None,
    ):
        """
        Extend the timestamp range of an existing parquet file by renaming it.

        This method looks for parquet files that are adjacent to the specified timestamp range
        and renames them to include the new range. It's useful for extending existing files
        without having to rewrite them when a query returns an empty list.

        Parameters
        ----------
        data_cls : type[Data]
            The data class type to extend files for.
        instrument_id : str, optional
            The instrument ID to filter files by. If None, applies to all instruments.
        start : int, optional
            The start timestamp (nanoseconds) of the new range.
        end : int, optional
            The end timestamp (nanoseconds) of the new range.

        Notes
        -----
        - Both `start` and `end` must be provided for the method to take effect.
        - The method only extends files if they are exactly adjacent to the new range
          (i.e., if `interval[0] == end + 1` or `interval[1] == start - 1`).
        - After renaming, the method verifies that the intervals remain disjoint.

        """
        if start is None or end is None:
            return

        directory = self._make_path(data_cls=data_cls, instrument_id=instrument_id)
        intervals = self._get_directory_intervals(directory)

        for interval in intervals:
            if interval[0] == end + 1:
                old_path = os.path.join(directory, f"{interval[0]}-{interval[1]}.parquet")
                new_path = os.path.join(directory, f"{start}-{interval[1]}.parquet")
                self.fs.rename(old_path, new_path)
                break
            elif interval[1] == start - 1:
                old_path = os.path.join(directory, f"{interval[0]}-{interval[1]}.parquet")
                new_path = os.path.join(directory, f"{interval[0]}-{end}.parquet")
                self.fs.rename(old_path, new_path)
                break

        intervals = self._get_directory_intervals(directory)
        assert _are_intervals_disjoint(
            intervals,
        ), "Intervals are not disjoint after extending file name"

    def reset_catalog_file_names(self) -> None:
        """
        Reset the filenames of all parquet files in the catalog to match their actual
        content timestamps.

        This method identifies all leaf directories in the catalog that contain parquet files
        and resets their filenames to accurately reflect the minimum and maximum timestamps of the data
        they contain. It does this by examining the parquet metadata for each file and renaming the file
        to follow the pattern '{first_timestamp}-{last_timestamp}.parquet'.

        This is useful when file names may have become inconsistent with their content, for example
        after manual file operations or data corruption. It ensures that query operations that rely on
        filename-based filtering will work correctly.

        Notes
        -----
        - This operation scans all parquet files in the catalog and may be resource-intensive for
          large catalogs.
        - The method does not modify the content of the files, only their names.
        - After renaming, the method verifies that the intervals represented by the filenames
          are disjoint (non-overlapping) to maintain data integrity.
        - This method is a convenience wrapper that calls `_reset_file_names` on each leaf directory.

        """
        leaf_directories = self._find_leaf_data_directories()

        for directory in leaf_directories:
            self._reset_file_names(directory)

    def reset_data_file_names(
        self,
        data_cls: type,
        instrument_id: str | None = None,
    ) -> None:
        """
        Reset the filenames of parquet files for a specific data class and instrument
        ID.

        This method resets the filenames of parquet files for the specified data class and
        instrument ID to accurately reflect the minimum and maximum timestamps of the data
        they contain. It examines the parquet metadata for each file and renames the file
        to follow the pattern '{first_timestamp}-{last_timestamp}.parquet'.

        Parameters
        ----------
        data_cls : type
            The data class type to reset filenames for (e.g., QuoteTick, TradeTick, Bar).
        instrument_id : str, optional
            The specific instrument ID to reset filenames for. If None, resets filenames
            for all instruments of the specified data class.

        Notes
        -----
        - This operation is more targeted than `reset_catalog_file_names` as it only affects
          files for a specific data class and instrument ID.
        - The method does not modify the content of the files, only their names.
        - After renaming, the method verifies that the intervals represented by the filenames
          are disjoint (non-overlapping) to maintain data integrity.
        - This method is useful for correcting filename inconsistencies for a specific data type
          without processing the entire catalog.

        """
        directory = self._make_path(data_cls, instrument_id)
        self._reset_file_names(directory)

    def _reset_file_names(self, directory: str) -> None:
        if not self.fs.exists(directory):
            return

        parquet_files = self.fs.glob(os.path.join(directory, "*.parquet"))

        for file in parquet_files:
            first_ts, last_ts = _min_max_from_parquet_metadata(file, "ts_init")

            if first_ts == -1:
                continue

            new_filename = f"{first_ts}-{last_ts}.parquet"
            new_path = os.path.join(os.path.dirname(file), new_filename)
            self.fs.rename(file, new_path)

        intervals = self._get_directory_intervals(directory)
        assert _are_intervals_disjoint(
            intervals,
        ), "Intervals are not disjoint after resetting file names"

    def consolidate_catalog(
        self,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
    ) -> None:
        """
        Consolidate all parquet files across the entire catalog within the specified
        time range.

        This method identifies all leaf directories in the catalog that contain parquet files
        and consolidates them. A leaf directory is one that contains files but no subdirectories.
        This is a convenience method that effectively calls `consolidate_data` for all data types
        and instrument IDs in the catalog.

        Parameters
        ----------
        start : TimestampLike, optional
            The start timestamp for the consolidation range. Only files with timestamps
            greater than or equal to this value will be consolidated. If None, all files
            from the beginning of time will be considered.
        end : TimestampLike, optional
            The end timestamp for the consolidation range. Only files with timestamps
            less than or equal to this value will be consolidated. If None, all files
            up to the end of time will be considered.

        Notes
        -----
        - This operation can be resource-intensive for large catalogs with many data types
          and instruments.
        - The consolidation process only combines files with non-overlapping timestamp ranges.
        - If timestamp ranges overlap between files in any directory, the consolidation for
          that directory will be aborted for safety.
        - After consolidation, the original files are removed and replaced with a single file
          in each leaf directory.
        - This method is useful for periodic maintenance of the catalog to improve query
          performance and reduce storage overhead.

        """
        leaf_directories = self._find_leaf_data_directories()

        for directory in leaf_directories:
            self._consolidate_directory(directory, start, end)

    def consolidate_data(
        self,
        data_cls: type,
        instrument_id: str | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
    ) -> None:
        """
        Consolidate multiple parquet files for a specific data class and instrument ID
        into a single file.

        This method identifies all parquet files within the specified time range for the given data class
        and instrument ID, then combines them into a single parquet file. This helps improve query
        performance and reduces storage overhead by eliminating small fragmented files.

        Parameters
        ----------
        data_cls : type
            The data class type to consolidate (e.g., QuoteTick, TradeTick, Bar).
        instrument_id : str, optional
            The specific instrument ID to consolidate data for. If None, consolidates data
            for all instruments of the specified data class.
        start : TimestampLike, optional
            The start timestamp for the consolidation range. Only files with timestamps
            greater than or equal to this value will be consolidated. If None, all files
            from the beginning of time will be considered.
        end : TimestampLike, optional
            The end timestamp for the consolidation range. Only files with timestamps
            less than or equal to this value will be consolidated. If None, all files
            up to the end of time will be considered.

        Notes
        -----
        - The consolidation process only combines files with non-overlapping timestamp ranges.
        - If timestamp ranges overlap between files, the consolidation will be aborted for safety.
        - The method uses the `_combine_data_files` function which sorts files by their first timestamp
          before combining them.
        - After consolidation, the original files are removed and replaced with a single file.

        """
        directory = self._make_path(data_cls, instrument_id)
        self._consolidate_directory(directory, start, end)

    def _consolidate_directory(
        self,
        directory: str,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        ensure_contiguous_files: bool = True,
    ) -> None:
        parquet_files = self.fs.glob(os.path.join(directory, "*.parquet"))
        files_to_consolidate = []
        used_start: pd.Timestamp | None = time_object_to_dt(start)
        used_end: pd.Timestamp | None = time_object_to_dt(end)
        intervals = []

        if len(parquet_files) <= 1:
            return

        for file in parquet_files:
            interval = _parse_filename_timestamps(file)

            if (
                interval
                and (used_start is None or interval[0] >= used_start.value)
                and (used_end is None or interval[1] <= used_end.value)
            ):
                files_to_consolidate.append(file)
                intervals.append(interval)

        intervals.sort(key=lambda x: x[0])

        if ensure_contiguous_files:
            assert _are_intervals_contiguous(intervals)

        new_file_name = os.path.join(directory, f"{intervals[0][0]}-{intervals[-1][1]}.parquet")
        files_to_consolidate.sort()
        self._combine_parquet_files(files_to_consolidate, new_file_name)

    def _combine_parquet_files(self, file_list: list[str], new_file: str) -> None:
        if len(file_list) <= 1:
            return

        tables = [pq.read_table(file, memory_map=True, pre_buffer=False) for file in file_list]
        combined_table = pa.concat_tables(tables)
        pq.write_table(combined_table, where=new_file)

        for file in file_list:
            self.fs.rm(file)

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
        data_lists = []

        for cls in subclasses:
            try:
                data_list = self.query(
                    data_cls=cls,
                    filter_expr=filter_expr,
                    instrument_ids=instrument_ids,
                    raise_on_empty=False,
                    **kwargs,
                )
                data_lists.append(data_list)
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

        non_empty_data_lists = [data_list for data_list in data_lists if data_list is not None]
        objects = [o for objs in non_empty_data_lists for o in objs]  # flatten of list of lists

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
        """
        Query the catalog for data matching the specified criteria.

        This method retrieves data from the catalog based on the provided filters.
        It automatically selects the appropriate query implementation (Rust or PyArrow)
        based on the data class and filesystem protocol.

        Parameters
        ----------
        data_cls : type
            The data class type to query for.
        instrument_ids : list[str], optional
            A list of instrument IDs to filter by. If None, all instruments are included.
        bar_types : list[str], optional
            A list of bar types to filter by (only applicable when querying Bar data).
            If None, all bar types are included.
        start : TimestampLike, optional
            The start timestamp for the query range. If None, no lower bound is applied.
        end : TimestampLike, optional
            The end timestamp for the query range. If None, no upper bound is applied.
        where : str, optional
            An additional SQL WHERE clause to filter the data (used in Rust queries).
        **kwargs : Any
            Additional keyword arguments passed to the underlying query implementation.

        Returns
        -------
        list[Data | CustomData]
            A list of data objects matching the query criteria.

        Notes
        -----
        - For Nautilus built-in data types (OrderBookDelta, QuoteTick, etc.) with the 'file'
          protocol, the Rust implementation is used for better performance.
        - For other data types or protocols, the PyArrow implementation is used.
        - Non-Nautilus data classes are wrapped in CustomData objects with the appropriate
          DataType.

        """
        if self.fs_protocol == "file" and data_cls in (
            OrderBookDelta,
            OrderBookDeltas,
            OrderBookDepth10,
            QuoteTick,
            TradeTick,
            Bar,
            MarkPriceUpdate,
        ):
            data = self._query_rust(
                data_cls=data_cls,
                instrument_ids=instrument_ids,
                bar_types=bar_types,
                start=start,
                end=end,
                where=where,
                **kwargs,
            )
        else:
            data = self._query_pyarrow(
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

    def _query_rust(
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
        """
        Create or update a DataBackendSession for querying data using the Rust backend.

        This method is used internally by the `query_rust` method to set up the query session.
        It identifies the relevant parquet files and adds them to the session with appropriate
        SQL queries.

        Parameters
        ----------
        data_cls : type
            The data class type to query for.
        instrument_ids : list[str], optional
            A list of instrument IDs to filter by. If None, all instruments are included.
        bar_types : list[str], optional
            A list of bar types to filter by (only applicable when querying Bar data).
            If None, all bar types are included.
        start : TimestampLike, optional
            The start timestamp for the query range. If None, no lower bound is applied.
        end : TimestampLike, optional
            The end timestamp for the query range. If None, no upper bound is applied.
        where : str, optional
            An additional SQL WHERE clause to filter the data.
        session : DataBackendSession, optional
            An existing session to update. If None, a new session is created.
        **kwargs : Any
            Additional keyword arguments.

        Returns
        -------
        DataBackendSession
            The updated or newly created session.

        Notes
        -----
        - This method only works with the 'file' protocol.
        - It maps the data class to the appropriate NautilusDataType for the Rust backend.
        - The method filters files by directory structure and filename patterns before adding
          them to the session.
        - Each file is added with a SQL query that includes the specified filters.

        Raises
        ------
        AssertionError
            If the filesystem protocol is not 'file'.
        RuntimeError
            If the data class is not supported by the Rust backend.

        """
        assert self.fs_protocol == "file", "Only file:// protocol is supported for Rust queries"
        data_type: NautilusDataType = ParquetDataCatalog._nautilus_data_cls_to_data_type(data_cls)
        files = self._query_files(data_cls, instrument_ids, bar_types, start, end)
        file_prefix = class_to_filename(data_cls)

        if session is None:
            session = DataBackendSession()

        for idx, file in enumerate(files):
            table = f"{file_prefix}_{idx}"
            query = self._build_query(
                table,
                # instrument_ids=None, # Filtering by filename for now
                start=start,
                end=end,
                where=where,
            )
            session.add_file(data_type, table, str(file), query)

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
        elif data_cls == MarkPriceUpdate:
            return NautilusDataType.MarkPriceUpdate
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

    def _query_pyarrow(
        self,
        data_cls: type,
        instrument_ids: list[str] | None = None,
        bar_types: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        filter_expr: str | None = None,
        **kwargs: Any,
    ) -> list[Data]:
        # Load dataset
        files = self._query_files(data_cls, instrument_ids, bar_types, start, end)

        if not files:
            return []

        dataset = pds.dataset(files, filesystem=self.fs)

        # Filter dataset
        used_start: pd.Timestamp | None = time_object_to_dt(start)
        used_end: pd.Timestamp | None = time_object_to_dt(end)
        filters: list[pds.Expression] = [filter_expr] if filter_expr is not None else []

        if used_start is not None:
            filters.append(pds.field("ts_init") >= used_start.value)

        if used_end is not None:
            filters.append(pds.field("ts_init") <= used_end.value)

        if filters:
            combined_filters = combine_filters(*filters)
        else:
            combined_filters = None

        table = dataset.to_table(filter=combined_filters)

        # Convert dataset to nautilus objects
        if table is None or table.num_rows == 0:
            return []

        return self._handle_table_nautilus(table, data_cls=data_cls)

    def _query_files(
        self,
        data_cls: type,
        instrument_ids: list[str] | None = None,
        bar_types: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
    ):
        file_prefix = class_to_filename(data_cls)
        glob_path = f"{self.path}/data/{file_prefix}/**/*.parquet"
        files: list[str] = self.fs.glob(glob_path)

        instrument_ids = instrument_ids or bar_types

        if instrument_ids:
            if not isinstance(instrument_ids, list):
                instrument_ids = [instrument_ids]

            files = [
                fn for fn in files if any(urisafe_instrument_id(x) in fn for x in instrument_ids)
            ]

        used_start: pd.Timestamp | None = time_object_to_dt(start)
        used_end: pd.Timestamp | None = time_object_to_dt(end)
        files = [fn for fn in files if _query_intersects_filename(fn, used_start, used_end)]

        if self.show_query_paths:
            for file in files:
                print(file)

        return files

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

    def query_last_timestamp(
        self,
        data_cls: type,
        instrument_id: str | None = None,
    ) -> pd.Timestamp | None:
        subclasses = [data_cls, *data_cls.__subclasses__()]

        for cls in subclasses:
            intervals = self.get_intervals(cls, instrument_id)

            if intervals:
                return time_object_to_dt(intervals[-1][1])

        return None

    def get_missing_intervals_for_request(
        self,
        start: int,
        end: int,
        data_cls: type,
        instrument_id: str | None = None,
    ) -> list[tuple[int, int]]:
        """
        Find the missing time intervals for a specific data class and instrument ID.

        This method identifies the gaps in the data between the specified start and end
        timestamps. It's useful for determining what data needs to be fetched or generated
        to complete a time series.

        Parameters
        ----------
        start : int
            The start timestamp (nanoseconds) of the request range.
        end : int
            The end timestamp (nanoseconds) of the request range.
        data_cls : type
            The data class type to check for.
        instrument_id : str, optional
            The instrument ID to check for. If None, checks across all instruments.

        Returns
        -------
        list[tuple[int, int]]
            A list of (start, end) timestamp tuples representing the missing intervals.
            Each tuple represents a continuous range of missing data.

        Notes
        -----
        - The method uses the filename patterns to determine the available data intervals.
        - It does not examine the actual content of the files.
        - The returned intervals are disjoint (non-overlapping) and sorted by start time.
        - If all data is available (no gaps), an empty list is returned.
        - If no data is available in the entire range, a single tuple (start, end) is returned.

        """
        intervals = self.get_intervals(data_cls, instrument_id)

        return _query_interval_diff(start, end, intervals)

    def get_intervals(
        self,
        data_cls: type,
        instrument_id: str | None = None,
    ) -> list[tuple[int, int]]:
        """
        Get the time intervals covered by parquet files for a specific data class and
        instrument ID.

        This method retrieves the timestamp ranges of all parquet files for the specified data class
        and instrument ID. Each parquet file in the catalog follows a naming convention of
        '{start_timestamp}-{end_timestamp}.parquet', which this method parses to determine
        the available data intervals.

        Parameters
        ----------
        data_cls : type
            The data class type to get intervals for.
        instrument_id : str, optional
            The instrument ID to get intervals for. If None, gets intervals across all instruments
            for the specified data class.

        Returns
        -------
        list[tuple[int, int]]
            A list of (start, end) timestamp tuples representing the available data intervals.
            Each tuple contains the start and end timestamps (in nanoseconds) of a continuous
            range of data. The intervals are sorted by start time.

        Notes
        -----
        - This method only examines the filenames and does not inspect the actual content of the files.
        - The returned intervals are sorted by start timestamp.
        - If no data is available, an empty list is returned.
        - This method is useful for determining what data is available before making queries.
        - Used internally by methods like `get_missing_intervals_for_request` and `_query_last_timestamp`.

        """
        directory = self._make_path(data_cls, instrument_id)

        return self._get_directory_intervals(directory)

    def _get_directory_intervals(self, directory: str) -> list[tuple[int, int]]:
        parquet_files = self.fs.glob(os.path.join(directory, "*.parquet"))
        intervals = []

        for file in parquet_files:
            interval = _parse_filename_timestamps(file)

            if interval:
                intervals.append(interval)

        intervals.sort(key=lambda x: x[0])

        return intervals

    def _make_path(
        self,
        data_cls: type[Data],
        instrument_id: str | None = None,
    ) -> str:
        file_prefix = class_to_filename(data_cls)
        directory = f"{self.path}/data/{file_prefix}"

        # instrument_id can be an instrument_id or a bar_type
        if instrument_id is not None:
            directory += f"/{urisafe_instrument_id(instrument_id)}"

        return directory

    # -- OVERLOADED BASE METHODS ------------------------------------------------------------------

    def _list_directory_stems(self, subdirectory: str) -> list[str]:
        glob_path = f"{self.path}/{subdirectory}/*"

        return [Path(p).stem for p in self.fs.glob(glob_path)]

    def list_data_types(self) -> list[str]:
        """
        List all data types available in the catalog.

        Returns
        -------
        list[str]
        A list of data type names (as directory stems) in the catalog.

        """
        return self._list_directory_stems("data")

    def list_backtest_runs(self) -> list[str]:
        """
        List all backtest run IDs available in the catalog.

        Returns
        -------
        list[str]
        A list of backtest run IDs (as directory stems) in the catalog.

        """
        return self._list_directory_stems("backtest")

    def list_live_runs(self) -> list[str]:
        """
        List all live run IDs available in the catalog.

        Returns
        -------
        list[str]
        A list of live run IDs (as directory stems) in the catalog.

        """
        return self._list_directory_stems("live")

    def read_live_run(self, instance_id: str, **kwargs: Any) -> list[Data]:
        """
        Read data from a live run.

        This method reads all data associated with a specific live run instance
        from feather files.

        Parameters
        ----------
        instance_id : str
            The ID of the live run instance.
        **kwargs : Any
            Additional keyword arguments passed to the underlying `_read_feather` method.

        Returns
        -------
        list[Data]
            A list of data objects from the live run, sorted by timestamp.

        """
        return self._read_feather(kind="live", instance_id=instance_id, **kwargs)

    def read_backtest(self, instance_id: str, **kwargs: Any) -> list[Data]:
        """
        Read data from a backtest run.

        This method reads all data associated with a specific backtest run instance
        from feather files.

        Parameters
        ----------
        instance_id : str
            The ID of the backtest run instance.
        **kwargs : Any
            Additional keyword arguments passed to the underlying `_read_feather` method.

        Returns
        -------
        list[Data]
            A list of data objects from the backtest run, sorted by timestamp.

        """
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
    ) -> None:
        """
        Convert stream data from feather files to parquet files.

        This method reads data from feather files generated during a backtest or live run
        and writes it to the catalog in parquet format. It's useful for converting temporary
        stream data into a more permanent and queryable format.

        Parameters
        ----------
        instance_id : str
            The ID of the backtest or live run instance.
        data_cls : type
            The data class type to convert.
        other_catalog : ParquetDataCatalog, optional
            An alternative catalog to write the data to. If None, writes to this catalog.
        subdirectory : str, default "backtest"
            The subdirectory containing the feather files. Either "backtest" or "live".

        Notes
        -----
        - The method looks for feather files in two possible locations:
          1. {path}/{subdirectory}/{instance_id}/{table_name}/*.feather
          2. {path}/{subdirectory}/{instance_id}/{table_name}_*.feather
        - It reads each feather file, deserializes the data, and collects it into a list.
        - The data is then sorted by timestamp and written to the catalog.
        - If no feather files are found or they contain no data, no action is taken.

        """
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
        used_catalog.write_data(all_data)


def _query_intersects_filename(
    filename: str,
    start: pd.Timestamp | None,
    end: pd.Timestamp | None,
) -> bool:
    file_interval = _parse_filename_timestamps(filename)

    if not file_interval:
        return True

    file_start, file_end = file_interval

    return (start is None or start.value <= file_end) and (end is None or file_start <= end.value)


def _parse_filename_timestamps(filename: str) -> tuple[int, int] | None:
    base_filename = os.path.splitext(os.path.basename(filename))[0]
    match = re.match(r"(\d+)-(\d+)", base_filename)

    if not match:
        return None

    first_ts = int(match.group(1))
    last_ts = int(match.group(2))

    return (first_ts, last_ts)


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


def _are_intervals_disjoint(intervals: list[tuple[int, int]]) -> bool:
    n = len(intervals)

    if n <= 1:
        return True

    union_interval = P.empty()

    for interval in intervals:
        union_interval |= P.closed(interval[0], interval[1])

    return len(union_interval) == n


def _are_intervals_contiguous(intervals: list[tuple[int, int]]) -> bool:
    n = len(intervals)

    if n <= 1:
        return True

    for i in range(1, n):
        if intervals[i - 1][1] + 1 != intervals[i][0]:
            return False

    return True


def _query_interval_diff(
    start: int,
    end: int,
    closed_intervals: list[tuple[int, int]],
) -> list[tuple[int, int]]:
    interval_set = _get_integer_interval_set(closed_intervals)
    interval_query = P.closed(start, end)
    interval_diff = interval_query - interval_set

    return [
        (interval.lower, interval.upper if interval.right == P.CLOSED else interval.upper - 1)
        for interval in interval_diff
    ]


# closed_intervals = [(1,2),(4,5), (10,12)]
# start = -1
# end = 15
# _query_interval_diff(start, end, closed_intervals)


# the idea is that for integer intervals [1,2], [3,4], representing them as [1,3[, [3, 5[
# allows to get a union as [1,5[ which is the same as [1,4] for integers
def _get_integer_interval_set(intervals: list[tuple[int, int]]) -> P.Interval:
    if not intervals:
        return P.empty()

    union_result = P.empty()

    for interval in intervals:
        union_result |= P.closedopen(interval[0], interval[1] + 1)

    return union_result
