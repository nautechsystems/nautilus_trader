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

import datetime
from typing import Any, BinaryIO, Optional

import fsspec
import pyarrow as pa
from pyarrow import RecordBatchStreamWriter

from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.model.data import GenericData
from nautilus_trader.persistence.catalog.parquet.util import GENERIC_DATA_PREFIX
from nautilus_trader.persistence.catalog.parquet.util import class_to_filename
from nautilus_trader.serialization.arrow.serializer import ArrowSerializer
from nautilus_trader.serialization.arrow.serializer import list_schemas
from nautilus_trader.serialization.arrow.serializer import register_arrow


class StreamingFeatherWriter:
    """
    Provides a stream writer of Nautilus objects into feather files.

    Parameters
    ----------
    path : str
        The path to persist the stream to.
    logger : LoggerAdapter
        The logger for the writer.
    fs_protocol : str, default 'file'
        The `fsspec` file system protocol.
    flush_interval_ms : int, optional
        The flush interval (milliseconds) for writing chunks.
    replace : bool, default False
        If existing files at the given `path` should be replaced.

    """

    def __init__(
        self,
        path: str,
        logger: LoggerAdapter,
        fs_protocol: Optional[str] = "file",
        flush_interval_ms: Optional[int] = None,
        replace: bool = False,
        include_types: Optional[tuple[type]] = None,
    ):
        self.fs: fsspec.AbstractFileSystem = fsspec.filesystem(fs_protocol)

        self.path = path

        self.fs.makedirs(self.fs._parent(self.path), exist_ok=True)

        err_dir_empty = "Path must be directory or empty"
        assert self.fs.isdir(self.path) or not self.fs.exists(self.path), err_dir_empty

        self.include_types = include_types
        if self.fs.exists(self.path) and replace:
            for fn in self.fs.ls(self.path):
                self.fs.rm(fn)
            self.fs.rmdir(self.path)

        self.fs.makedirs(self.fs._parent(self.path), exist_ok=True)

        self._schemas = list_schemas()
        self.logger = logger
        self._files: dict[str, BinaryIO] = {}
        self._writers: dict[str, RecordBatchStreamWriter] = {}
        self._create_writers()

        self.flush_interval_ms = datetime.timedelta(milliseconds=flush_interval_ms or 1000)
        self._last_flush = datetime.datetime(1970, 1, 1)  # Default value to begin
        self.missing_writers: set[type] = set()

    def _create_writer(self, cls):
        if self.include_types is not None and cls.__name__ not in self.include_types:
            return
        table_name = class_to_filename(cls)
        if table_name in self._writers:
            return
        prefix = GENERIC_DATA_PREFIX if not is_nautilus_class(cls) else ""
        schema = self._schemas[cls]
        full_path = f"{self.path}/{prefix}{table_name}.feather"

        self.fs.makedirs(self.fs._parent(full_path), exist_ok=True)
        f = self.fs.open(full_path, "wb")
        self._files[cls] = f

        self._writers[table_name] = pa.ipc.new_stream(f, schema)

    def _create_writers(self):
        for cls in self._schemas:
            self._create_writer(cls=cls)

    @property
    def closed(self) -> bool:
        return all(self._files[cls].closed for cls in self._files)

    def write(self, obj: object) -> None:
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
        if isinstance(obj, GenericData):
            cls = obj.data_type.type
        table = class_to_filename(cls)
        if table not in self._writers:
            if table.startswith("Signal"):
                self._create_writer(cls=cls)
            elif cls not in self.missing_writers:
                self.logger.warning(f"Can't find writer for cls: {cls}")
                self.missing_writers.add(cls)
                return
            else:
                return
        writer: RecordBatchStreamWriter = self._writers[table]
        serialized = ArrowSerializer.serialize_batch([obj], cls=cls)
        if not serialized:
            return
        try:
            for batch in serialized.to_batches():
                writer.write_batch(batch)
            self.check_flush()
        except Exception as e:
            self.logger.error(f"Failed to serialize {cls=}")
            self.logger.error(f"ERROR = `{e}`")
            self.logger.debug(f"data = {obj}")

    def check_flush(self) -> None:
        """
        Flush all stream writers if current time greater than the next flush interval.
        """
        now = datetime.datetime.now()
        if now - self._last_flush > self.flush_interval_ms:
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
        for cls in tuple(self._writers):
            self._writers[cls].close()
            del self._writers[cls]
        for cls in self._files:
            self._files[cls].close()


def generate_signal_class(name: str, value_type: type) -> type:
    """
    Dynamically create a Data subclass for this signal.

    Parameters
    ----------
    name : str
        The name of the signal data.
    value_type : type
        The type for the signal data value.

    Returns
    -------
    SignalData

    """

    class SignalData(Data):
        """
        Represents generic signal data.
        """

        def __init__(self, value: Any, ts_event: int, ts_init: int) -> None:
            self.value = value
            self._ts_event = ts_event
            self._ts_init = ts_init

        @property
        def ts_event(self) -> int:
            """
            The UNIX timestamp (nanoseconds) when the data event occurred.

            Returns
            -------
            int

            """
            return self._ts_event

        @property
        def ts_init(self) -> int:
            """
            The UNIX timestamp (nanoseconds) when the object was initialized.

            Returns
            -------
            int

            """
            return self._ts_init

    SignalData.__name__ = f"Signal{name.title()}"

    # Parquet serialization
    def serialize_signal(data: list) -> pa.Table:
        return pa.Table.from_pylist(
            [
                {
                    "ts_init": d.ts_init,
                    "ts_event": d.ts_event,
                    "value": d.value,
                }
                for d in data
            ],
            schema=schema,
        )

    def deserialize_signal(data):
        return SignalData(**data)

    schema = pa.schema(
        {
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "value": {int: pa.int64(), float: pa.float64(), str: pa.string()}[value_type],
        },
    )
    register_arrow(
        cls=SignalData,
        serializer=serialize_signal,
        deserializer=deserialize_signal,
        schema=schema,
    )

    return SignalData
