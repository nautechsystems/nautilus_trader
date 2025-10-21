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
from nautilus_trader.core.datetime import maybe_dt_to_unix_nanos
from nautilus_trader.core.datetime import time_object_to_dt
from nautilus_trader.core.datetime import unix_nanos_to_iso8601
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
from nautilus_trader.persistence.funcs import filename_to_class
from nautilus_trader.persistence.funcs import urisafe_identifier
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
    fs_rust_storage_options : dict[str, str], optional
        Storage-specific configuration options for the rust backend.
        Defaults to what is used for fs_storage_options if not specified.
    max_rows_per_group : int, default 5000
        The maximum number of rows per group. If the value is greater than 0,
        then the dataset writer may split up large incoming batches into
        multiple row groups.
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
        fs_rust_storage_options: dict | None = None,
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
        self.fs_rust_storage_options = fs_rust_storage_options or self.fs_storage_options
        self.serializer = ArrowSerializer()
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
    def from_uri(
        cls,
        uri: str,
        fs_storage_options: dict[str, str] | None = None,
        fs_rust_storage_options: dict[str, str] | None = None,
    ) -> ParquetDataCatalog:
        """
        Create a data catalog instance from the given `uri` with optional storage
        options.

        Parameters
        ----------
        uri : str
            The URI string for the backing path.
        fs_storage_options : dict[str, str], optional
            Storage-specific configuration options.
            For S3: endpoint_url, region, access_key_id, secret_access_key, session_token, etc.
            For GCS: service_account_path, service_account_key, project_id, etc.
            For Azure: account_name, account_key, sas_token, etc.
        fs_rust_storage_options : dict[str, str], optional
            Storage-specific configuration options for the rust backend.
            Defaults to what is used for fs_storage_options if not specified.

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

        # Merge parsed storage options with provided storage options
        # Provided storage options take precedence
        merged_fs_storage_options = parsed.copy()

        if fs_storage_options:
            merged_fs_storage_options.update(fs_storage_options)

        return cls(
            path=path,
            fs_protocol=protocol,
            fs_storage_options=merged_fs_storage_options,
            fs_rust_storage_options=fs_rust_storage_options,
        )

    # -- WRITING ----------------------------------------------------------------------------------

    def write_data(
        self,
        data: list[Data | Event] | list[NautilusRustDataType],
        start: int | None = None,
        end: int | None = None,
        skip_disjoint_check: bool = False,
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
        skip_disjoint_check : bool, default False
            If True, skip the disjoint intervals check.

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

        def identifier_function(obj: Any) -> tuple[str, str | None]:
            if isinstance(obj, CustomData):
                obj = obj.data

            # Class name of an object
            name = type(obj).__name__

            # Identifier
            if isinstance(obj, Instrument):
                return name, obj.id.value
            elif hasattr(obj, "bar_type"):
                return name, str(obj.bar_type)
            elif hasattr(obj, "instrument_id"):
                return name, obj.instrument_id.value

            # Custom data case without instrument_id
            return name, None

        def obj_to_type(obj: Data) -> type:
            return type(obj) if not isinstance(obj, CustomData) else obj.data.__class__

        name_to_cls = {cls.__name__: cls for cls in {obj_to_type(d) for d in data}}

        for (cls_name, identifier), single_type_data in groupby(
            sorted(data, key=identifier_function),
            key=identifier_function,
        ):
            chunk = list(single_type_data)
            self._write_chunk(
                data=chunk,
                data_cls=name_to_cls[cls_name],
                identifier=identifier,
                start=start,
                end=end,
                skip_disjoint_check=skip_disjoint_check,
            )

    def _write_chunk(
        self,
        data: list[Data],
        data_cls: type[Data],
        identifier: str | None = None,
        start: int | None = None,
        end: int | None = None,
        skip_disjoint_check: bool = False,
    ) -> None:
        if isinstance(data[0], CustomData):
            data = [d.data for d in data]

        table = self._objects_to_table(data, data_cls=data_cls)
        directory = self._make_path(data_cls=data_cls, identifier=identifier)
        self.fs.mkdirs(directory, exist_ok=True)

        start = start if start else data[0].ts_init
        end = end if end else data[-1].ts_init
        filename = _timestamps_to_filename(start, end)
        parquet_file = f"{directory}/{filename}"
        pq.write_table(
            table,
            where=parquet_file,
            filesystem=self.fs,
            row_group_size=self.max_rows_per_group,
        )

        if not skip_disjoint_check:
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
        identifier: str | None = None,
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
        identifier : str, optional
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

        directory = self._make_path(data_cls=data_cls, identifier=identifier)
        intervals = self._get_directory_intervals(directory)

        for interval in intervals:
            if interval[0] == end + 1:
                old_path = os.path.join(
                    directory,
                    _timestamps_to_filename(interval[0], interval[1]),
                )
                new_path = os.path.join(directory, _timestamps_to_filename(start, interval[1]))
                self.fs.rename(old_path, new_path)
                break
            elif interval[1] == start - 1:
                old_path = os.path.join(
                    directory,
                    _timestamps_to_filename(interval[0], interval[1]),
                )
                new_path = os.path.join(directory, _timestamps_to_filename(interval[0], end))
                self.fs.rename(old_path, new_path)
                break

        intervals = self._get_directory_intervals(directory)
        assert _are_intervals_disjoint(
            intervals,
        ), "Intervals are not disjoint after extending file name"

    def reset_all_file_names(self) -> None:
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
        identifier: str | None = None,
    ) -> None:
        """
        Reset the filenames of parquet files for a specific data class and instrument
        ID.

        This method resets the filenames of parquet files for the specified data class and
        identifier to accurately reflect the minimum and maximum timestamps of the data
        they contain. It examines the parquet metadata for each file and renames the file
        to follow the pattern '{first_timestamp}-{last_timestamp}.parquet'.

        Parameters
        ----------
        data_cls : type
            The data class type to reset filenames for (e.g., QuoteTick, TradeTick, Bar).
        identifier : str, optional
            The specific identifier (instrument ID, etc) to reset filenames for.
            If None, resets filenames for all instruments of the specified data class.

        Notes
        -----
        - This operation is more targeted than `reset_all_file_names` as it only affects
          files for a specific data class and identifier.
        - The method does not modify the content of the files, only their names.
        - After renaming, the method verifies that the intervals represented by the filenames
          are disjoint (non-overlapping) to maintain data integrity.
        - This method is useful for correcting filename inconsistencies for a specific data type
          without processing the entire catalog.

        """
        directory = self._make_path(data_cls, identifier)
        self._reset_file_names(directory)

    def _reset_file_names(self, directory: str) -> None:
        if not self.fs.exists(directory):
            return

        parquet_files = self.fs.glob(os.path.join(directory, "*.parquet"))

        for file in parquet_files:
            first_ts, last_ts = self._min_max_from_parquet_metadata(file, "ts_init")

            if first_ts == -1:
                continue

            new_filename = _timestamps_to_filename(first_ts, last_ts)
            new_path = os.path.join(os.path.dirname(file), new_filename)
            self.fs.rename(file, new_path)

        intervals = self._get_directory_intervals(directory)
        assert _are_intervals_disjoint(
            intervals,
        ), "Intervals are not disjoint after resetting file names"

    def _min_max_from_parquet_metadata(self, file_path: str, column_name: str) -> tuple[int, int]:
        parquet_file = pq.ParquetFile(file_path, filesystem=self.fs)
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

        if overall_min_value is None or overall_max_value is None:
            print(f"Column '{column_name}' not found or has no statistics in any row group.")
            return -1, -1
        else:
            return overall_min_value, overall_max_value

    def consolidate_catalog(
        self,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        ensure_contiguous_files: bool = True,
        deduplicate: bool = False,
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
        ensure_contiguous_files : bool, default True
            If True, ensures that files have contiguous timestamps before consolidation.
        deduplicate : bool, default False
            If True, removes duplicate rows from the consolidated file.

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
            self._consolidate_directory(
                directory,
                start,
                end,
                ensure_contiguous_files,
                deduplicate=deduplicate,
            )

    def consolidate_data(
        self,
        data_cls: type,
        identifier: str | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        ensure_contiguous_files: bool = True,
        deduplicate: bool = False,
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
        identifier : str, optional
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
        ensure_contiguous_files : bool, default True
            If True, ensures that files have contiguous timestamps before consolidation.
        deduplicate : bool, default False
            If True, removes duplicate rows from the consolidated file.

        Notes
        -----
        - The consolidation process only combines files with non-overlapping timestamp ranges.
        - If timestamp ranges overlap between files, the consolidation will be aborted for safety.
        - The method uses the `_combine_data_files` function which sorts files by their first timestamp
          before combining them.
        - After consolidation, the original files are removed and replaced with a single file.

        """
        directory = self._make_path(data_cls, identifier)
        self._consolidate_directory(
            directory,
            start,
            end,
            ensure_contiguous_files,
            deduplicate=deduplicate,
        )

    def _consolidate_directory(
        self,
        directory: str,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        ensure_contiguous_files: bool = True,
        deduplicate: bool = False,
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
                and (used_start is None or used_start.value <= interval[0])
                and (used_end is None or interval[1] <= used_end.value)
            ):
                files_to_consolidate.append(file)
                intervals.append(interval)

        intervals.sort(key=lambda x: x[0])

        if ensure_contiguous_files:
            assert _are_intervals_contiguous(intervals)

        new_file_name = os.path.join(
            directory,
            _timestamps_to_filename(intervals[0][0], intervals[-1][1]),
        )
        files_to_consolidate.sort()
        self._combine_parquet_files(files_to_consolidate, new_file_name, deduplicate=deduplicate)

    def _combine_parquet_files(
        self,
        file_list: list[str],
        new_file: str,
        deduplicate: bool = False,
    ) -> None:
        if len(file_list) <= 1:
            return

        tables = [
            pq.read_table(file, memory_map=True, pre_buffer=False, filesystem=self.fs)
            for file in file_list
        ]
        combined_table = pa.concat_tables(tables)

        if deduplicate:
            combined_table = self._deduplicate_table(combined_table)

        pq.write_table(combined_table, where=new_file, filesystem=self.fs)

        for file in file_list:
            if file != new_file:
                self.fs.rm(file)

    @staticmethod
    def _deduplicate_table(table: pa.Table) -> pa.Table:
        deduped_data_table = table.group_by(table.column_names).aggregate([])
        return pa.Table.from_arrays(
            deduped_data_table.columns,
            schema=table.schema,
        )

    def consolidate_catalog_by_period(
        self,
        period: pd.Timedelta = pd.Timedelta(days=1),
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        ensure_contiguous_files: bool = True,
    ) -> None:
        """
        Consolidate all parquet files across the entire catalog by splitting them into
        fixed time periods.

        This method identifies all leaf directories in the catalog that contain parquet files
        and consolidates them by period. A leaf directory is one that contains files but no subdirectories.
        This is a convenience method that effectively calls `consolidate_data_by_period` for all data types
        and instrument IDs in the catalog.

        Parameters
        ----------
        period : pd.Timedelta, default pd.Timedelta(days=1)
            The period duration for consolidation. Default is 1 day.
            Examples: pd.Timedelta(hours=1), pd.Timedelta(days=7), pd.Timedelta(minutes=30)
        start : TimestampLike, optional
            The start timestamp for the consolidation range. Only files with timestamps
            greater than or equal to this value will be consolidated. If None, all files
            from the beginning of time will be considered.
        end : TimestampLike, optional
            The end timestamp for the consolidation range. Only files with timestamps
            less than or equal to this value will be consolidated. If None, all files
            up to the end of time will be considered.
        ensure_contiguous_files : bool, default True
            If True, uses period boundaries for file naming.
            If False, uses actual data timestamps for file naming.

        Notes
        -----
        - This operation can be resource-intensive for large catalogs with many data types
          and instruments.
        - The consolidation process splits data into fixed time periods rather than combining
          all files into a single file per directory.
        - Uses the same period-based consolidation logic as `consolidate_data_by_period`.
        - Original files are removed and replaced with period-based consolidated files.
        - This method is useful for periodic maintenance of the catalog to standardize
          file organization by time periods.

        """
        leaf_directories = self._find_leaf_data_directories()

        for directory in leaf_directories:
            data_cls, identifier = self._extract_data_cls_and_identifier_from_path(directory)

            if data_cls is None:
                # Skip directories that don't correspond to known data classes
                return

            # Call the existing consolidate_data_by_period method
            self.consolidate_data_by_period(
                data_cls=data_cls,
                identifier=identifier,
                period=period,
                start=start,
                end=end,
                ensure_contiguous_files=ensure_contiguous_files,
            )

    def consolidate_data_by_period(  # noqa: C901
        self,
        data_cls: type,
        identifier: str | None = None,
        period: pd.Timedelta = pd.Timedelta(days=1),
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        ensure_contiguous_files: bool = True,
    ) -> None:
        """
        Consolidate data files by splitting them into fixed time periods.

        This method queries data by period and writes consolidated files immediately,
        using the skip_disjoint_check parameter to avoid interval conflicts during
        the consolidation process. When start/end boundaries intersect existing files,
        the function automatically splits those files to preserve all data.

        Parameters
        ----------
        data_cls : type
            The data class type to consolidate.
        identifier : str, optional
            The instrument ID to consolidate. If None, consolidates all instruments.
        period : pd.Timedelta, default pd.Timedelta(days=1)
            The period duration for consolidation. Default is 1 day.
            Examples: pd.Timedelta(hours=1), pd.Timedelta(days=7), pd.Timedelta(minutes=30)
        start : TimestampLike, optional
            The start timestamp for consolidation range. If None, uses earliest available data.
            If specified and intersects existing files, those files will be split to preserve
            data outside the consolidation range.
        end : TimestampLike, optional
            The end timestamp for consolidation range. If None, uses latest available data.
            If specified and intersects existing files, those files will be split to preserve
            data outside the consolidation range.
        ensure_contiguous_files : bool, default True
            If True, uses period boundaries for file naming.
            If False, uses actual data timestamps for file naming.

        Notes
        -----
        - Uses two-phase approach: first determines all queries, then executes them
        - Groups intervals into contiguous groups to preserve holes between groups
        - Allows consolidation across multiple files within each contiguous group
        - Skips queries if target files already exist for efficiency
        - Original files are removed immediately after querying each period
        - Uses skip_disjoint_check to avoid interval conflicts during consolidation
        - When ensure_contiguous_files=False, file timestamps match actual data range
        - When ensure_contiguous_files=True, file timestamps use period boundaries
        - Uses modulo arithmetic for efficient period boundary calculation
        - Preserves holes in data by preventing queries from spanning across gaps
        - Automatically splits files at start/end boundaries to preserve all data
        - Split operations are executed before consolidation to ensure data preservation

        """
        # Use get_intervals for cleaner implementation
        intervals = self.get_intervals(data_cls, identifier)

        if not intervals:
            return  # No files to consolidate

        # Use auxiliary function to prepare all queries for execution
        queries_to_execute = self._prepare_consolidation_queries(
            data_cls,
            identifier,
            intervals,
            period,
            start,
            end,
            ensure_contiguous_files,
        )

        if not queries_to_execute:
            return  # No queries to execute

        # Get directory for file operations
        directory = self._make_path(data_cls, identifier)
        existing_files = sorted(self.fs.glob(os.path.join(directory, "*.parquet")))

        # Track files to remove and maintain existing_files list
        files_to_remove = set()
        existing_files = list(existing_files)  # Make it mutable

        # Phase 2: Execute queries, write, and delete
        file_start_ns = None  # Track contiguity across periods

        for query_info in queries_to_execute:
            # Query data for this period using existing files
            period_data = self.query(
                data_cls=data_cls,
                identifiers=[identifier] if identifier is not None else None,
                start=query_info["query_start"],
                end=query_info["query_end"],
                files=existing_files,
            )

            if not period_data:
                # Skip if no data found, but maintain contiguity by using query start
                if file_start_ns is None:
                    file_start_ns = query_info["query_start"]

                continue
            else:
                file_start_ns = None

            # Determine final file timestamps
            if query_info["use_period_boundaries"]:
                # Use period boundaries for file naming, maintaining contiguity
                if file_start_ns is None:
                    file_start_ns = query_info["query_start"]

                file_end_ns = query_info["query_end"]
            else:
                # Use actual data timestamps for file naming
                file_start_ns = period_data[0].ts_init
                file_end_ns = period_data[-1].ts_init

            # Check again if target file exists (in case it was created during this process)
            target_filename = os.path.join(
                directory,
                _timestamps_to_filename(file_start_ns, file_end_ns),
            )

            if self.fs.exists(target_filename):
                # Skip if target file already exists
                continue

            # Write consolidated data for this period
            # Use skip_disjoint_check since we're managing file removal carefully
            self.write_data(
                data=period_data,
                start=file_start_ns,
                end=file_end_ns,
                skip_disjoint_check=True,
            )

            # Clear the data from memory immediately
            del period_data

            # Identify files that are completely covered by this period
            for file in existing_files[:]:  # Use slice copy to avoid modification during iteration
                interval = _parse_filename_timestamps(file)

                if interval and interval[1] <= query_info["query_end"]:
                    files_to_remove.add(file)
                    existing_files.remove(file)

            # Remove files as soon as we have some to remove
            if files_to_remove:
                for file in list(files_to_remove):  # Copy to avoid modification during iteration
                    self.fs.rm(file)
                    files_to_remove.remove(file)

        # Remove any remaining files that weren't removed in the loop
        for file in existing_files:
            self.fs.rm(file)

    def _prepare_consolidation_queries(  # noqa: C901
        self,
        data_cls: type,
        identifier: str | None,
        intervals: list[tuple[int, int]],
        period: pd.Timedelta,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        ensure_contiguous_files: bool = True,
    ) -> list[dict]:
        """
        Prepare all queries for consolidation by filtering, grouping, and handling
        splits.

        This auxiliary function handles all the preparation logic for consolidation:
        1. Filters intervals by time range
        2. Groups intervals into contiguous groups
        3. Identifies and creates split operations for data preservation
        4. Generates period-based consolidation queries
        5. Checks for existing target files

        Parameters
        ----------
        data_cls : type
            The data class type for path generation
        identifier : str, optional
            The instrument identifier for path generation
        intervals : list[tuple[int, int]]
            List of (start_ts, end_ts) tuples representing existing file intervals
        period : pd.Timedelta
            The period duration for consolidation
        start : TimestampLike, optional
            The start timestamp for consolidation range
        end : TimestampLike, optional
            The end timestamp for consolidation range
        ensure_contiguous_files : bool, default True
            If True, uses period boundaries for file naming

        Returns
        -------
        list[dict]
            List of query dictionaries ready for execution

        """
        # Filter intervals by time range if specified
        used_start: pd.Timestamp | None = time_object_to_dt(start)
        used_end: pd.Timestamp | None = time_object_to_dt(end)

        filtered_intervals = []

        for interval_start, interval_end in intervals:
            # Check if interval overlaps with the specified range
            if (used_start is None or used_start.value <= interval_end) and (
                used_end is None or interval_start <= used_end.value
            ):
                filtered_intervals.append((interval_start, interval_end))

        if not filtered_intervals:
            return []  # No intervals in the specified range

        # Check contiguity of filtered intervals if required
        if ensure_contiguous_files:
            assert _are_intervals_contiguous(filtered_intervals), (
                "Intervals are not contiguous. When ensure_contiguous_files=True, "
                "all files in the consolidation range must have contiguous timestamps."
            )

        # Group intervals into contiguous groups to preserve holes between groups
        # but allow consolidation within each contiguous group
        contiguous_groups = []
        current_group = [filtered_intervals[0]]

        for i in range(1, len(filtered_intervals)):
            prev_interval = filtered_intervals[i - 1]
            curr_interval = filtered_intervals[i]

            # Check if current interval is contiguous with previous (end + 1 == start)
            if prev_interval[1] + 1 == curr_interval[0]:
                current_group.append(curr_interval)
            else:
                # Gap found, start new group
                contiguous_groups.append(current_group)
                current_group = [curr_interval]

        # Add the last group
        contiguous_groups.append(current_group)

        # Convert period to nanoseconds for calculations
        period_in_ns = period.value

        # Start with split queries for data preservation
        queries_to_execute = []

        # Handle interval splitting by creating split operations for data preservation
        if filtered_intervals and used_start is not None:
            first_interval = filtered_intervals[0]

            if first_interval[0] < used_start.value <= first_interval[1]:
                # Split before start: preserve data from interval_start to start-1
                queries_to_execute.append(
                    {
                        "query_start": first_interval[0],
                        "query_end": used_start.value - 1,
                        "use_period_boundaries": False,
                    },
                )

        if filtered_intervals and used_end is not None:
            last_interval = filtered_intervals[-1]

            if last_interval[0] <= used_end.value < last_interval[1]:
                # Split after end: preserve data from end+1 to interval_end
                queries_to_execute.append(
                    {
                        "query_start": used_end.value + 1,
                        "query_end": last_interval[1],
                        "use_period_boundaries": False,
                    },
                )

        directory = self._make_path(data_cls, identifier)

        # Generate period-based consolidation queries for each contiguous group
        for group in contiguous_groups:
            # Get overall time range for this contiguous group
            group_start_ts = group[0][0]
            group_end_ts = group[-1][1]

            # Apply user-provided start/end constraints to this group
            if used_start is not None:
                group_start_ts = max(group_start_ts, used_start.value)

            if used_end is not None:
                group_end_ts = min(group_end_ts, used_end.value)

            # Skip group if constraints make it invalid
            if group_start_ts > group_end_ts:
                continue

            # Calculate period boundaries for this group using modulo arithmetic
            period_start_ns = (group_start_ts // period_in_ns) * period_in_ns
            current_start_ns = period_start_ns

            # Safety check to prevent infinite loops
            max_iterations = 10000  # Reasonable upper bound
            iteration_count = 0

            while current_start_ns <= group_end_ts:
                iteration_count += 1

                if iteration_count > max_iterations:
                    # Safety break to prevent infinite loops
                    break

                current_end_ns = current_start_ns + period_in_ns - 1

                # Adjust end to not exceed the group end timestamp
                if current_end_ns > group_end_ts:
                    current_end_ns = group_end_ts

                # Create target filename to check if it already exists (only for period boundaries)
                if ensure_contiguous_files:
                    target_filename = os.path.join(
                        directory,
                        _timestamps_to_filename(current_start_ns, current_end_ns),
                    )

                    # Skip if target file already exists
                    if self.fs.exists(target_filename):
                        current_start_ns += period_in_ns
                        continue

                # Add query to execution list
                queries_to_execute.append(
                    {
                        "query_start": current_start_ns,
                        "query_end": current_end_ns,
                        "use_period_boundaries": ensure_contiguous_files,
                    },
                )

                # Move to next period
                current_start_ns += period_in_ns

                if current_start_ns > group_end_ts:
                    break

        # Sort queries by start date to enable efficient file removal
        # Files can be removed when interval[1] <= query_info["query_end"]
        # and processing in chronological order ensures optimal cleanup
        return sorted(queries_to_execute, key=lambda q: q["query_start"])

    def delete_catalog_range(
        self,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
    ) -> None:
        """
        Delete data within a specified time range across the entire catalog.

        This method identifies all leaf directories in the catalog that contain parquet files
        and deletes data within the specified time range from each directory. A leaf directory
        is one that contains files but no subdirectories. This is a convenience method that
        effectively calls `delete_data_range` for all data types and instrument IDs in the catalog.

        Parameters
        ----------
        start : TimestampLike, optional
            The start timestamp for the deletion range. If None, deletes from the beginning.
        end : TimestampLike, optional
            The end timestamp for the deletion range. If None, deletes to the end.

        Notes
        -----
        - This operation permanently removes data and cannot be undone
        - The deletion process handles file intersections intelligently by splitting files
          when they partially overlap with the deletion range
        - Files completely within the deletion range are removed entirely
        - Files partially overlapping the deletion range are split to preserve data outside the range
        - This method is useful for bulk data cleanup operations across the entire catalog
        - Empty directories are not automatically removed after deletion

        """
        leaf_directories = self._find_leaf_data_directories()

        for directory in leaf_directories:
            # Extract data class and identifier from directory path
            try:
                data_cls, identifier = self._extract_data_cls_and_identifier_from_path(directory)
                if data_cls is not None:
                    self.delete_data_range(data_cls, identifier, start, end)
            except Exception as e:
                print(f"Failed to delete data in directory {directory}: {e}")
                continue

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

    def _extract_data_cls_and_identifier_from_path(
        self,
        directory: str,
    ) -> tuple[type | None, str | None]:
        # Remove the base catalog path to get the relative path
        base_path = self.path.rstrip("/")

        if directory.startswith(base_path):
            relative_path = directory[len(base_path) :].lstrip("/")
        else:
            relative_path = directory

        # Expected format: "data/{data_type_filename}/{identifier}" or "data/{data_type_filename}"
        path_parts = relative_path.split("/")

        if len(path_parts) < 2 or path_parts[0] != "data":
            return None, None

        data_type_filename = path_parts[1]
        identifier = path_parts[2] if len(path_parts) > 2 else None

        # Convert filename back to data class
        data_cls = filename_to_class(data_type_filename)

        return data_cls, identifier

    def delete_data_range(  # noqa: C901
        self,
        data_cls: type,
        identifier: str | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
    ) -> None:
        """
        Delete data within a specified time range for a specific data class and
        instrument.

        This method identifies all parquet files that intersect with the specified time range
        and handles them appropriately:
        - Files completely within the range are deleted
        - Files partially overlapping the range are split to preserve data outside the range
        - The original intersecting files are removed after processing

        Parameters
        ----------
        data_cls : type
            The data class type to delete data for (e.g., QuoteTick, TradeTick, Bar).
        identifier : str, optional
            The instrument identifier to delete data for. If None, deletes data across all instruments
            for the specified data class.
        start : TimestampLike, optional
            The start timestamp for the deletion range. If None, deletes from the beginning.
        end : TimestampLike, optional
            The end timestamp for the deletion range. If None, deletes to the end.

        Notes
        -----
        - This operation permanently removes data and cannot be undone
        - Files that partially overlap the deletion range are split to preserve data outside the range
        - The method ensures data integrity by using atomic operations where possible
        - Empty directories are not automatically removed after deletion

        """
        # Handle identifier=None by deleting from all identifiers for this data class
        if identifier is None:
            # Find all directories for this data class
            leaf_directories = self._find_leaf_data_directories()
            data_cls_name = class_to_filename(data_cls)

            for directory in leaf_directories:
                # Check if this directory is for the specified data class
                if f"/data/{data_cls_name}/" in directory:
                    # Extract the identifier from the directory path
                    parts = directory.split("/")

                    if len(parts) >= 3 and parts[-2] == data_cls_name:
                        dir_identifier = parts[-1]
                        # Recursively call delete for this specific identifier
                        self.delete_data_range(
                            data_cls=data_cls,
                            identifier=dir_identifier,
                            start=start,
                            end=end,
                        )
            return

        # Use get_intervals for cleaner implementation
        intervals = self.get_intervals(data_cls, identifier)

        if not intervals:
            return  # No files to process

        # Use auxiliary function to prepare all operations for execution
        operations_to_execute = self._prepare_delete_operations(
            data_cls,
            identifier,
            intervals,
            start,
            end,
        )

        if not operations_to_execute:
            return  # No operations to execute

        # Execute all operations
        files_to_remove = set()

        for operation in operations_to_execute:
            if operation["type"] == "split_before":
                # Query data before the deletion range and write it
                before_data = self.query(
                    data_cls=data_cls,
                    identifiers=[identifier] if identifier else None,
                    start=operation["query_start"],
                    end=operation["query_end"],
                    files=operation["files"],
                )

                if before_data:
                    self.write_data(
                        data=before_data,
                        start=operation["file_start_ns"],
                        end=operation["file_end_ns"],
                        skip_disjoint_check=True,
                    )

            elif operation["type"] == "split_after":
                # Query data after the deletion range and write it
                after_data = self.query(
                    data_cls=data_cls,
                    identifiers=[identifier] if identifier else None,
                    start=operation["query_start"],
                    end=operation["query_end"],
                    files=operation["files"],
                )

                if after_data:
                    self.write_data(
                        data=after_data,
                        start=operation["file_start_ns"],
                        end=operation["file_end_ns"],
                        skip_disjoint_check=True,
                    )

            # Mark files for removal (applies to all operation types)
            for file in operation["files"]:
                files_to_remove.add(file)

        # Remove all files that were processed
        for file in files_to_remove:
            if self.fs.exists(file):
                self.fs.rm(file)

    def _prepare_delete_operations(
        self,
        data_cls: type,
        identifier: str | None,
        intervals: list[tuple[int, int]],
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
    ) -> list[dict[str, Any]]:
        """
        Prepare all operations for data deletion by identifying files that need to be
        split or removed.

        This auxiliary function handles all the preparation logic for deletion:
        1. Filters intervals by time range
        2. Identifies files that intersect with the deletion range
        3. Creates split operations for files that partially overlap
        4. Generates removal operations for files completely within the range

        Parameters
        ----------
        data_cls : type
            The data class type for path generation
        identifier : str, optional
            The instrument identifier for path generation
        intervals : list[tuple[int, int]]
            List of (start_ts, end_ts) tuples representing existing file intervals
        start : TimestampLike, optional
            The start timestamp for deletion range
        end : TimestampLike, optional
            The end timestamp for deletion range

        Returns
        -------
        list[dict]
            List of operation dictionaries ready for execution

        """
        # Convert start/end to nanoseconds
        used_start: pd.Timestamp | None = time_object_to_dt(start)
        used_end: pd.Timestamp | None = time_object_to_dt(end)

        delete_start_ns = used_start.value if used_start else None
        delete_end_ns = used_end.value if used_end else None

        operations: list[dict[str, Any]] = []

        # Get all files for this data class and identifier
        all_files = self._query_files(data_cls, [identifier] if identifier else None)

        for file in all_files:
            interval = _parse_filename_timestamps(file)
            if not interval:
                continue

            file_start_ns, file_end_ns = interval

            # Check if file intersects with deletion range
            intersects = (delete_start_ns is None or delete_start_ns <= file_end_ns) and (
                delete_end_ns is None or file_start_ns <= delete_end_ns
            )

            if not intersects:
                continue  # File doesn't intersect with deletion range

            # Determine what type of operation is needed
            file_completely_within_range = (
                delete_start_ns is None or delete_start_ns <= file_start_ns
            ) and (delete_end_ns is None or file_end_ns <= delete_end_ns)

            if file_completely_within_range:
                # File is completely within deletion range - just mark for removal
                operations.append(
                    {
                        "type": "remove",
                        "files": [file],
                    },
                )
            else:
                # File partially overlaps - need to split
                if delete_start_ns is not None and file_start_ns < delete_start_ns:
                    # Keep data before deletion range
                    operations.append(
                        {
                            "type": "split_before",
                            "files": [file],
                            "query_start": file_start_ns,
                            "query_end": delete_start_ns - 1,  # Exclusive end
                            "file_start_ns": file_start_ns,
                            "file_end_ns": delete_start_ns - 1,
                        },
                    )

                if delete_end_ns is not None and delete_end_ns < file_end_ns:
                    # Keep data after deletion range
                    operations.append(
                        {
                            "type": "split_after",
                            "files": [file],
                            "query_start": delete_end_ns + 1,  # Exclusive start
                            "query_end": file_end_ns,
                            "file_start_ns": delete_end_ns + 1,
                            "file_end_ns": file_end_ns,
                        },
                    )

        return operations

    # -- QUERIES ----------------------------------------------------------------------------------

    def _query_subclasses(
        self,
        base_cls: type,
        identifiers: list[str] | None = None,
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
                    identifiers=identifiers,
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
        identifiers: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        where: str | None = None,
        files: list[str] | None = None,
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
        identifiers : list[str], optional
            A list of instrument IDs to filter by. If None, all instruments are included.
        start : TimestampLike, optional
            The start timestamp for the query range. If None, no lower bound is applied.
        end : TimestampLike, optional
            The end timestamp for the query range. If None, no upper bound is applied.
        where : str, optional
            An additional SQL WHERE clause to filter the data (used in Rust queries).
        files : list[str], optional
            A specific list of files to query from. If provided, these files are used
            instead of discovering files through the normal process.
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
        - When files parameter is provided, PyArrow backend is used regardless of data type.
        - Non-Nautilus data classes are wrapped in CustomData objects with the appropriate
          DataType.

        """
        if (
            data_cls
            in (
                OrderBookDelta,
                OrderBookDeltas,
                OrderBookDepth10,
                QuoteTick,
                TradeTick,
                Bar,
                MarkPriceUpdate,
            )
            and files is None
        ):  # Rust backend doesn't support custom files yet
            data = self._query_rust(
                data_cls=data_cls,
                identifiers=identifiers,
                start=start,
                end=end,
                where=where,
                files=files,
                **kwargs,
            )
        else:
            data = self._query_pyarrow(
                data_cls=data_cls,
                identifiers=identifiers,
                start=start,
                end=end,
                where=where,
                files=files,
                **kwargs,
            )

        if not is_nautilus_class(data_cls):
            # Special handling for generic data
            metadata = kwargs.get("metadata")

            if callable(metadata):
                data = [
                    CustomData(data_type=DataType(data_cls, metadata=metadata(d)), data=d)
                    for d in data
                ]
            else:
                data = [
                    CustomData(data_type=DataType(data_cls, metadata=metadata), data=d)
                    for d in data
                ]

        return data

    def _query_rust(
        self,
        data_cls: type,
        identifiers: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        where: str | None = None,
        files: list[str] | None = None,
        **kwargs: Any,
    ) -> list[Data]:
        query_data_cls = OrderBookDelta if data_cls == OrderBookDeltas else data_cls
        session = self.backend_session(
            data_cls=query_data_cls,
            identifiers=identifiers,
            start=start,
            end=end,
            where=where,
            file=files,
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
        identifiers: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        where: str | None = None,
        session: DataBackendSession | None = None,
        files: list[str] | None = None,
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
        identifiers : list[str], optional
            A list of instrument IDs to filter by. If None, all instruments are included.
        start : TimestampLike, optional
            The start timestamp for the query range. If None, no lower bound is applied.
        end : TimestampLike, optional
            The end timestamp for the query range. If None, no upper bound is applied.
        where : str, optional
            An additional SQL WHERE clause to filter the data.
        session : DataBackendSession, optional
            An existing session to update. If None, a new session is created.
        files : list[str], optional
            A specific list of files to query from. If provided, these files are used
            instead of discovering files through the normal process.
        **kwargs : Any
            Additional keyword arguments.

        Returns
        -------
        DataBackendSession
            The updated or newly created session.

        Notes
        -----
        - It maps the data class to the appropriate NautilusDataType for the Rust backend.
        - The method filters files by directory structure and filename patterns before adding
          them to the session.
        - Each file is added with a SQL query that includes the specified filters.
        - Supports various object store backends including local files, AWS S3, Google Cloud Storage,
          Azure Blob Storage, and HTTP/WebDAV servers.

        Raises
        ------
        RuntimeError
            If the data class is not supported by the Rust backend.

        """
        data_type: NautilusDataType = ParquetDataCatalog._nautilus_data_cls_to_data_type(data_cls)
        file_list = files if files else self._query_files(data_cls, identifiers, start, end)
        file_prefix = class_to_filename(data_cls)

        if session is None:
            session = DataBackendSession()

        # Register object store with the session for non-file protocols
        if self.fs_protocol != "file":
            self._register_object_store_with_session(session)

        for file in file_list:
            # Extract identifier from file path and filename to create meaningful table names
            identifier = file.split("/")[-2]
            safe_sql_identifier = (
                urisafe_identifier(identifier)
                .replace(".", "_")
                .replace("-", "_")
                .replace(" ", "_")
                .replace("^", "_")
                .lower()
            )
            safe_filename = _extract_sql_safe_filename(file)
            table = f"{file_prefix}_{safe_sql_identifier}_{safe_filename}"
            query = self._build_query(
                table,
                start=start,
                end=end,
                where=where,
            )

            file_uri = self._build_file_uri(file)

            session.add_file(data_type, table, file_uri, query)

        return session

    def _build_file_uri(self, file: str) -> str:
        """
        Convert a file path to a URI format based on the filesystem protocol.

        Parameters
        ----------
        file : str
            The file path to convert.

        Returns
        -------
        str
            The file path in URI format.

        """
        if self.fs_protocol != "file" and "://" not in file:
            # Convert relative paths to full URIs based on protocol
            if self.fs_protocol == "s3":
                return f"s3://{file}"
            elif self.fs_protocol in ("gcs", "gs"):
                return f"gs://{file}"
            elif self.fs_protocol in ("abfs"):
                return f"{self.path}/{file.partition('/')[2]}"
            elif self.fs_protocol in ("azure", "az"):
                return f"az://{file}"
            elif self.fs_protocol in ("http", "https"):
                return f"{self.fs_protocol}://{file}"
            # Add more protocols as needed
        elif self.fs_protocol == "file" and not file.startswith("file://"):
            # For local files, DataFusion can handle both absolute paths and file:// URIs
            # We'll keep the original path format for compatibility
            return file

        return file

    def _register_object_store_with_session(self, session: DataBackendSession) -> None:
        """
        Register object store with the DataFusion session for cloud storage access.

        This method creates and registers appropriate object store instances based on the
        filesystem protocol, enabling DataFusion to access cloud storage directly.

        Parameters
        ----------
        session : DataBackendSession
            The DataFusion session to register the object store with.

        """
        # Convert the catalog path to a URI for object store registration
        if "://" not in self.path:
            # Convert local-style paths to proper URIs based on protocol
            if self.fs_protocol == "s3":
                catalog_uri = f"s3://{self.path}"
            elif self.fs_protocol in ("gcs", "gs"):
                catalog_uri = f"gs://{self.path}"
            elif self.fs_protocol in ("abfs"):
                catalog_uri = f"abfs://{self.path}"
            elif self.fs_protocol in ("azure", "az"):
                catalog_uri = f"az://{self.path}"
            elif self.fs_protocol in ("http", "https"):
                catalog_uri = f"{self.fs_protocol}://{self.path}"
            else:
                # For unknown protocols, assume it's already a valid URI
                catalog_uri = self.path
        else:
            catalog_uri = self.path

        try:
            # Register object store using the Rust implementation with storage options
            session.register_object_store_from_uri(catalog_uri, self.fs_rust_storage_options)

        except Exception as e:
            # Log the error but don't fail - DataFusion might still work with built-in support
            import warnings

            warnings.warn(
                f"Failed to register object store for {catalog_uri}: {e}. "
                f"Falling back to DataFusion's built-in object store support.",
                UserWarning,
                stacklevel=2,
            )

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
        identifiers: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
        filter_expr: str | None = None,
        files: list[str] | None = None,
        **kwargs: Any,
    ) -> list[Data]:
        # Load dataset - use provided files or query for them
        file_list = files if files else self._query_files(data_cls, identifiers, start, end)

        if not file_list:
            return []

        dataset = pds.dataset(file_list, filesystem=self.fs)

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
        identifiers: list[str] | None = None,
        start: TimestampLike | None = None,
        end: TimestampLike | None = None,
    ):
        file_prefix = class_to_filename(data_cls)
        base_path = self.path.rstrip("/")
        glob_path = f"{base_path}/data/{file_prefix}/**/*.parquet"
        file_paths: list[str] = self.fs.glob(glob_path)

        if identifiers:
            if not isinstance(identifiers, list):
                identifiers = [identifiers]

            safe_identifiers = [urisafe_identifier(identifier) for identifier in identifiers]

            # Exact match by default for instrument_ids or bar_types
            exact_match_file_paths = [
                file_path
                for file_path in file_paths
                if any(
                    safe_identifier == file_path.split("/")[-2]
                    for safe_identifier in safe_identifiers
                )
            ]

            if not exact_match_file_paths and data_cls in [Bar, *Bar.__subclasses__()]:
                # Partial match of instrument_ids in bar_types for bars
                file_paths = [
                    file_path
                    for file_path in file_paths
                    if any(
                        file_path.split("/")[-2].startswith(f"{safe_identifier}-")
                        for safe_identifier in safe_identifiers
                    )
                ]
            else:
                file_paths = exact_match_file_paths

        used_start: pd.Timestamp | None = time_object_to_dt(start)
        used_end: pd.Timestamp | None = time_object_to_dt(end)
        file_paths = [
            file_path
            for file_path in file_paths
            if _query_intersects_filename(file_path, used_start, used_end)
        ]

        if self.show_query_paths:
            for file_path in file_paths:
                print(file_path)

        return file_paths

    @staticmethod
    def _handle_table_nautilus(
        table: pa.Table | pd.DataFrame,
        data_cls: type,
        convert_bar_type_to_external: bool = False,
    ) -> list[Data]:
        if isinstance(table, pd.DataFrame):
            table = pa.Table.from_pandas(table)

        # Convert metadata from INTERNAL to EXTERNAL if requested
        if convert_bar_type_to_external and table.schema.metadata:
            metadata = dict(table.schema.metadata)

            # Convert bar_type metadata (for Bar data)
            if b"bar_type" in metadata:
                bar_type_str = metadata[b"bar_type"].decode()

                if bar_type_str.endswith("-INTERNAL"):
                    metadata[b"bar_type"] = bar_type_str.replace("-INTERNAL", "-EXTERNAL").encode()

            # Replace schema with updated metadata (shallow copy)
            table = table.replace_schema_metadata(metadata)

        data = ArrowSerializer.deserialize(data_cls=data_cls, batch=table)
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
        identifier: str | None = None,
    ) -> pd.Timestamp | None:
        subclasses = [data_cls, *data_cls.__subclasses__()]

        for cls in subclasses:
            intervals = self.get_intervals(cls, identifier)

            if intervals:
                return time_object_to_dt(intervals[-1][1])

        return None

    def get_missing_intervals_for_request(
        self,
        start: int,
        end: int,
        data_cls: type,
        identifier: str | None = None,
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
        identifier : str, optional
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
        intervals = self.get_intervals(data_cls, identifier)

        return _query_interval_diff(start, end, intervals)

    def get_intervals(
        self,
        data_cls: type,
        identifier: str | None = None,
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
        identifier : str, optional
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
        directory = self._make_path(data_cls, identifier)

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
        identifier: str | None = None,
    ) -> str:
        file_prefix = class_to_filename(data_cls)
        # Remove trailing slash from path to avoid double slashes
        base_path = self.path.rstrip("/")
        directory = f"{base_path}/data/{file_prefix}"

        # Identifier can be an instrument_id or a bar_type
        if identifier is not None:
            directory += f"/{urisafe_identifier(identifier)}"

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
        prefix = f"{self.path}/{kind}/{urisafe_identifier(instance_id)}"

        # Non-instrument feather files
        for path_str in self.fs.glob(f"{prefix}/*.feather"):
            if not self.fs.isfile(path_str):
                continue

            file_name = path_str.replace(prefix + "/", "").replace(".feather", "")
            cls_name = "_".join(file_name.split("_")[:-1])

            if not cls_name:
                raise ValueError(f"`cls_name` was empty when a value was expected: {path_str}")

            yield FeatherFile(path=path_str, class_name=cls_name)

        # Per-instrument feather files (organized in subdirectories)
        for path_str in self.fs.glob(f"{prefix}/**/*.feather"):
            if not self.fs.isfile(path_str):
                continue

            file_name = path_str.replace(prefix + "/", "").replace(".feather", "")
            path_parts = Path(file_name).parts

            if len(path_parts) >= 2:
                cls_name = path_parts[0]  # cls_name is the first directory
            else:
                continue

            if not cls_name:
                continue

            yield FeatherFile(path=path_str, class_name=cls_name)

    def convert_stream_to_data(
        self,
        instance_id: str,
        data_cls: type,
        other_catalog: ParquetDataCatalog | None = None,
        subdirectory: str = "backtest",
        identifiers: list[str] | None = None,
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
        identifiers : list[str], optional
            Filter to only include data containing these identifiers in their instrument_ids or bar_types.

        """
        feather_dir = Path(self.path) / subdirectory / instance_id
        data_name = class_to_filename(data_cls)
        data_dir = feather_dir / data_name

        if self.fs.isdir(str(data_dir)):
            sub_dirs = [d for d in self.fs.glob(str(data_dir / "*")) if self.fs.isdir(d)]
            feather_files = []

            if not identifiers:
                for sub_dir in sub_dirs:
                    feather_files.extend(sorted(self.fs.glob(str(Path(sub_dir) / "*.feather"))))
            else:
                for sub_dir in sub_dirs:
                    sub_dir_name = Path(sub_dir).name

                    for identifier in identifiers:
                        if identifier in sub_dir_name:
                            feather_files.extend(
                                sorted(self.fs.glob(str(Path(sub_dir) / "*.feather"))),
                            )
        else:
            # Data is in flat files (old format or non-per-instrument data)
            feather_files = sorted(self.fs.glob(f"{feather_dir}/{data_name}_*.feather"))

        used_catalog = self if other_catalog is None else other_catalog

        for feather_file in feather_files:
            feather_table = self._read_feather_file(str(feather_file))

            if feather_table is None:
                continue

            file_data = self._handle_table_nautilus(
                feather_table,
                data_cls,
                convert_bar_type_to_external=True,
            )
            used_catalog.write_data(file_data)

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


def _timestamps_to_filename(timestamp_1: int, timestamp_2: int) -> str:
    datetime_1 = _iso_timestamp_to_file_timestamp(unix_nanos_to_iso8601(timestamp_1))
    datetime_2 = _iso_timestamp_to_file_timestamp(unix_nanos_to_iso8601(timestamp_2))

    return f"{datetime_1}_{datetime_2}.parquet"


def _iso_timestamp_to_file_timestamp(iso_timestamp: str) -> str:
    # Assumes format YYYY-MM-DDTHH:MM:SS.nanosecondsZ, "2023-10-26T07:30:50.123456789Z" becomes "2023-10-26T07-30-50-123456789Z"
    return iso_timestamp.replace(":", "-").replace(".", "-")


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
    match = re.match(r"(.*?)_(.*)", base_filename)

    if not match:
        return None

    first_ts = maybe_dt_to_unix_nanos(_file_timestamp_to_iso_timestamp(match.group(1)))
    last_ts = maybe_dt_to_unix_nanos(_file_timestamp_to_iso_timestamp(match.group(2)))

    # Note: only for linter
    if first_ts is None or last_ts is None:
        return None

    return (first_ts, last_ts)


def _file_timestamp_to_iso_timestamp(file_timestamp: str) -> str:
    # Assumes format YYYY-MM-DDTHH-MM-SS-nanosecondsZ, "2023-10-26T07-30-50-123456789Z" becomes "2023-10-26T07:30:50.123456789Z"
    date_part, time_part = file_timestamp.split("T")
    time_part = time_part[:-1]
    last_hyphen_idx = time_part.rfind("-")
    time_with_dot_for_nanos = time_part[:last_hyphen_idx] + "." + time_part[last_hyphen_idx + 1 :]
    final_time_part = time_with_dot_for_nanos.replace("-", ":")

    return f"{date_part}T{final_time_part}Z"


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


def _extract_sql_safe_filename(file_path: str) -> str:
    if not file_path:
        return "unknown_file"

    filename = file_path.split("/")[-1]

    return (
        filename.replace(".parquet", "")
        .replace("-", "_")
        .replace(":", "_")
        .replace(".", "_")
        .lower()
    )
