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

import datetime as dt
from enum import Enum
from io import TextIOWrapper
from typing import Any, BinaryIO

import fsspec
import pandas as pd
import pyarrow as pa
import pytz
from fsspec.compression import AbstractBufferedFile
from pyarrow import RecordBatchStreamWriter

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import Clock
from nautilus_trader.common.component import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.persistence.funcs import class_to_filename
from nautilus_trader.persistence.funcs import urisafe_instrument_id
from nautilus_trader.serialization.arrow.serializer import ArrowSerializer
from nautilus_trader.serialization.arrow.serializer import list_schemas


class RotationMode(Enum):
    SIZE = 0
    INTERVAL = 1
    SCHEDULED_DATES = 2
    NO_ROTATION = 3


class StreamingFeatherWriter:
    """
    Provides a stream writer of Nautilus objects into feather files with rotation
    capabilities.

    Parameters
    ----------
    path : str
        The path to persist the stream to. Must be a directory.
    cache : Cache
        The cache for the query info.
    clock : Clock
        The clock to use for time-related operations.
    fs_protocol : str, default 'file'
        The `fsspec` file system protocol.
    flush_interval_ms : int, optional
        The flush interval (milliseconds) for writing chunks.
    replace : bool, default False
        If existing files at the given `path` should be replaced.
    include_types : list[type], optional
        A list of Arrow serializable types to write.
        If this is specified then **only** the included types will be written.
    rotation_mode : RotationMode, default `RotationMode.NO_ROTATION`
        The mode for file rotation.
    max_file_size : int, default 1GB
        The maximum file size in bytes before rotation (for `SIZE` mode).
    rotation_interval : pd.Timedelta, default 1 day
        The time interval for file rotation (for `INTERVAL` mode and `SCHEDULED_DATES` mode).
    rotation_time : datetime.time, default 00:00
        The time of day for file rotation (for `SCHEDULED_DATES` mode).
    rotation_timezone : str, default 'UTC'
        The timezone for rotation calculations(for `SCHEDULED_DATES` mode).

    """

    def __init__(
        self,
        path: str,
        cache: Cache,
        clock: Clock,
        fs_protocol: str | None = "file",
        flush_interval_ms: int | None = None,
        replace: bool = False,
        include_types: list[type] | None = None,
        rotation_mode: RotationMode = RotationMode.NO_ROTATION,
        max_file_size: int = 1024 * 1024 * 1024,  # 1GB
        rotation_interval: pd.Timedelta = pd.Timedelta(days=1),
        rotation_time: dt.time = dt.time(0, 0, 0, 0),
        rotation_timezone: str = "UTC",
    ) -> None:
        self.path = path
        self.cache = cache
        self.clock = clock
        self.fs: fsspec.AbstractFileSystem = fsspec.filesystem(fs_protocol)
        self.fs.makedirs(self.fs._parent(self.path), exist_ok=True)

        if self.fs.exists(self.path):
            if not self.fs.isdir(self.path):
                raise FileNotFoundError("Path must be directory or empty")
        else:
            self.fs.makedirs(self.path, exist_ok=True)  # Create directory if it doesn't exist

        self.include_types = include_types
        if self.fs.exists(self.path) and replace:
            for fn in self.fs.ls(self.path):
                self.fs.rm(fn)
            self.fs.rmdir(self.path)

        self._schemas = list_schemas()
        self.logger = Logger(type(self).__name__)
        self._files: dict[
            str | tuple[str, str],
            TextIOWrapper | BinaryIO | AbstractBufferedFile,
        ] = {}
        self._writers: dict[str | tuple[str, str], RecordBatchStreamWriter] = {}
        self._instrument_writers: dict[tuple[str, str], RecordBatchStreamWriter] = {}
        self._per_instrument_writers = {
            "order_book_delta",
            "quote_tick",
            "trade_tick",
        }
        self.rotation_mode = rotation_mode
        self.max_file_size = max_file_size
        self.rotation_interval = rotation_interval
        self.rotation_time = rotation_time
        self.rotation_timezone = pytz.timezone(rotation_timezone)
        self._file_sizes: dict[str | tuple[str, str], int] = {}
        self._file_creation_times: dict[str | tuple[str, str], pd.Timestamp] = {}
        self._next_rotation_times: dict[str | tuple[str, str], pd.Timestamp | None] = {}

        self._create_writers()

        self.flush_interval_ms = flush_interval_ms or 1000
        self._last_flush = self.clock.utc_now()
        self.missing_writers: set[type] = set()

    def _update_next_rotation_time(self, table_name: str | tuple[str, str]) -> None:
        """
        Update the next rotation time for a specific table based on the current rotation
        mode and clock.
        """
        now = self.clock.utc_now()
        if self.rotation_mode == RotationMode.INTERVAL:
            self._next_rotation_times[table_name] = now + self.rotation_interval
        elif self.rotation_mode == RotationMode.SCHEDULED_DATES:
            if (
                table_name not in self._next_rotation_times
                or self._next_rotation_times[table_name] is None
            ):
                user_rotation_time = pd.Timestamp.combine(now.date(), self.rotation_time)
                next_rotation_time = pd.Timestamp(
                    user_rotation_time,
                    tz=self.rotation_timezone,
                ).tz_convert("UTC")
                while next_rotation_time <= now:
                    next_rotation_time += self.rotation_interval
                self._next_rotation_times[table_name] = next_rotation_time
            else:
                self._next_rotation_times[table_name] = now + self.rotation_interval
        elif self.rotation_mode in (RotationMode.SIZE, RotationMode.NO_ROTATION):
            self._next_rotation_times[table_name] = None

    def _check_file_rotation(self, table_name: str | tuple[str, str]) -> bool:
        """
        Check if file rotation is needed for the given table.

        Parameters
        ----------
        table_name : str | tuple[str, str]
            The name of the table to check.

        Returns
        -------
        bool
            True if rotation is needed, False otherwise.

        """
        if self.rotation_mode == RotationMode.NO_ROTATION:
            return False
        elif self.rotation_mode == RotationMode.SIZE:
            return self._file_sizes.get(table_name, 0) >= self.max_file_size
        elif self.rotation_mode in (RotationMode.INTERVAL, RotationMode.SCHEDULED_DATES):
            now = self.clock.utc_now()
            next_rotation_time = self._next_rotation_times.get(table_name)
            if next_rotation_time is None:
                self._update_next_rotation_time(table_name)
                return False
            elif now >= next_rotation_time:
                self._update_next_rotation_time(table_name)
                return True
        return False

    def _rotate_regular_file(self, table_name: str, cls: type) -> None:
        """
        Rotate the file for a regular table.

        Parameters
        ----------
        table_name : str
            The name of the table to rotate.
        cls : type
            The class type for the writer.

        """
        if table_name in self._writers:
            self._files[table_name].flush()
            self._writers[table_name].close()
            self._files[table_name].close()
            del self._writers[table_name]
            del self._files[table_name]

        self._create_writer(cls=cls, table_name=table_name)
        self._file_sizes[table_name] = 0
        self._file_creation_times[table_name] = self.clock.utc_now()
        self.logger.info(f"Rotated regular file for table '{table_name}'")

    def _rotate_per_instrument_file(self, cls: type, obj: Any) -> None:
        """
        Rotate the file for a per-instrument table.

        Parameters
        ----------
        cls : type
            The class type of the object.
        obj : Any
            The object containing instrument data.

        """
        table_name = class_to_filename(cls)
        key = (table_name, obj.instrument_id.value)
        if key in self._instrument_writers:
            self._files[key].flush()
            self._instrument_writers[key].close()
            self._files[key].close()
            del self._instrument_writers[key]
            del self._files[key]

        self._create_instrument_writer(cls=cls, obj=obj)

        self._file_sizes[key] = 0
        self._file_creation_times[key] = self.clock.utc_now()
        self.logger.info(
            f"Rotated instrument file for table '{table_name}' with instrument ID '{obj.instrument_id.value}'",
        )

    def _create_writer(self, cls: type, table_name: str | None = None) -> None:
        # Check if an include types filter has been specified
        if self.include_types is not None and cls not in self.include_types:
            return

        table_name = class_to_filename(cls) if not table_name else table_name

        if table_name in self._writers:
            return
        if table_name in self._per_instrument_writers:
            return

        schema = self._schemas[cls]
        timestamp = self.clock.timestamp_ns()
        full_path = f"{self.path}/{table_name}_{timestamp}.feather"
        print(full_path)

        self.fs.makedirs(self.fs._parent(full_path), exist_ok=True)
        f = self.fs.open(full_path, "wb")
        self._files[table_name] = f
        self._writers[table_name] = pa.ipc.new_stream(f, schema)
        self._file_sizes[table_name] = 0
        self._file_creation_times[table_name] = self.clock.utc_now()

        self.logger.info(f"Created writer for table '{table_name}'")

    def _create_writers(self) -> None:
        for cls in self._schemas:
            self._create_writer(cls=cls)

    def _create_instrument_writer(self, cls: type, obj: Any) -> None:
        # Check if an include types filter has been specified
        if self.include_types is not None and cls not in self.include_types:
            return

        # Create an arrow writer with instrument specific metadata in the schema
        metadata: dict[bytes, bytes] = self._extract_obj_metadata(obj)
        mapped_cls = {OrderBookDeltas: OrderBookDelta}.get(cls, cls)
        schema = self._schemas[mapped_cls].with_metadata(metadata)
        table_name = class_to_filename(cls)
        folder = f"{self.path}/{table_name}"
        key = (table_name, obj.instrument_id.value)
        self.fs.makedirs(folder, exist_ok=True)

        timestamp = self.clock.timestamp_ns()
        full_path = f"{folder}/{urisafe_instrument_id(obj.instrument_id.value)}_{timestamp}.feather"

        f = self.fs.open(full_path, "wb")
        self._files[key] = f
        self._instrument_writers[key] = pa.ipc.new_stream(f, schema)
        self._file_sizes[key] = 0
        self._file_creation_times[key] = self.clock.utc_now()
        self.logger.info(f"Created writer for table '{table_name}'")

    def _extract_obj_metadata(
        self,
        obj: TradeTick | QuoteTick | Bar | OrderBookDelta,
    ) -> dict[bytes, bytes]:
        instrument = self.cache.instrument(obj.instrument_id)
        metadata = {b"instrument_id": obj.instrument_id.value.encode()}
        if (
            isinstance(obj, OrderBookDelta)
            or isinstance(obj, OrderBookDeltas)
            or isinstance(obj, QuoteTick | TradeTick)
        ):
            metadata.update(
                {
                    b"price_precision": str(instrument.price_precision).encode(),
                    b"size_precision": str(instrument.size_precision).encode(),
                },
            )
        elif isinstance(obj, Bar):
            metadata.update(
                {
                    b"bar_type": str(obj.bar_type).encode(),
                    b"price_precision": str(instrument.price_precision).encode(),
                    b"size_precision": str(instrument.size_precision).encode(),
                },
            )
        else:
            raise NotImplementedError(
                f"type '{(type(obj)).__name__}' not currently supported for writing feather files.",
            )

        return metadata

    def write(self, obj: object) -> None:  # noqa: C901
        """
        Write the object to the stream.

        Parameters
        ----------
        obj : object
            The object to write.

        Raises
        ------
        ValueError
            If `obj` is ``None``.

        """
        PyCondition.not_none(obj, "obj")

        cls = obj.__class__

        # Check if an include types filter has been specified
        if self.include_types is not None and cls not in self.include_types:
            return

        if isinstance(obj, CustomData):
            cls = obj.data_type.type

        table = class_to_filename(cls)
        if isinstance(obj, Bar):
            bar: Bar = obj
            table += f"_{str(bar.bar_type).lower()}"

        if table not in self._writers:
            self.logger.debug(f"Writer not setup for table '{table}'")
            if table.startswith("custom_signal"):
                self._create_writer(cls=cls)
            elif table.startswith(("bar", "binance_bar")):
                self._create_writer(cls=cls, table_name=table)
            elif table in self._per_instrument_writers:
                key = (table, obj.instrument_id.value)  # type: ignore
                instrument = self.cache.instrument(obj.instrument_id)  # type: ignore
                if key not in self._instrument_writers and instrument is not None:
                    self._create_instrument_writer(cls=cls, obj=obj)
            elif cls not in self.missing_writers:
                self.logger.warning(f"Can't find writer for cls: {cls}")
                self.missing_writers.add(cls)
                return
            else:
                return

        if table in self._per_instrument_writers:
            key = (table, obj.instrument_id.value)  # type: ignore
            if key in self._instrument_writers:
                writer: RecordBatchStreamWriter = self._instrument_writers[key]
            else:
                return
        else:
            writer: RecordBatchStreamWriter = self._writers[table]  # type: ignore

        serialized = ArrowSerializer.serialize_batch([obj], data_cls=cls)
        if not serialized:
            return
        try:
            writer.write_table(serialized)
            self._file_sizes[table] = self._file_sizes.get(table, 0) + serialized.nbytes
            self.check_flush()
            if self._check_file_rotation(table):
                if table in self._per_instrument_writers:
                    self._rotate_per_instrument_file(cls=cls, obj=obj)
                else:
                    self._rotate_regular_file(table, cls)
        except Exception as e:
            self.logger.error(f"Failed to serialize {cls=}")
            self.logger.error(f"ERROR = `{e}`")
            self.logger.debug(f"data = {obj}")

    def check_flush(self) -> None:
        """
        Flush all stream writers if current time greater than the next flush interval.
        """
        now = self.clock.utc_now()
        if (now - self._last_flush).total_seconds() * 1000 > self.flush_interval_ms:
            self.flush()
            self._last_flush = now

    def flush(self) -> None:
        """
        Flush all stream writers.
        """
        for stream in self._files.values():
            if not stream.closed:
                stream.flush()

    def close(self) -> None:
        """
        Flush and close all stream writers.
        """
        self.flush()
        for wcls in tuple(self._writers):
            self._writers[wcls].close()
            del self._writers[wcls]
        for fcls in self._files:
            self._files[fcls].close()

    def get_current_file_info(self) -> dict[str | tuple[str, str], dict[str, Any]]:
        """
        Get information about the current files being written.

        Returns
        -------
        dict[str | tuple[str, str], dict[str, Any]]
            A dictionary containing file information for each table.

        """
        return {
            table_name: {
                "size": self._file_sizes.get(table_name, 0),
                "creation_time": self._file_creation_times.get(table_name),
            }
            for table_name in self._writers
        }

    def get_next_rotation_time(
        self,
        table_name: str | tuple[str, str],
    ) -> pd.Timestamp | None:
        """
        Get the expected time for the next file rotation.

        Parameters
        ----------
        table_name : str | tuple[str, str]
            The specific table name to get the next rotation time for.

        Returns
        -------
        pd.Timestamp | None
            The next rotation time for the specified table, or None if not set.

        """
        return self._next_rotation_times.get(table_name)

    @property
    def is_closed(self) -> bool:
        """
        Return whether all file streams are closed.

        Returns
        -------
        bool
            True if all streams are closed, False otherwise.

        """
        return all(self._files[table_name].closed for table_name in self._files)
