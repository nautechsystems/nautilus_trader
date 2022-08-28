# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
import pathlib
from typing import BinaryIO, Dict, Optional, Set, Tuple

import fsspec
import pyarrow as pa
from pyarrow import RecordBatchStreamWriter

from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.persistence.catalog.parquet import resolve_path
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.serializer import get_cls_table
from nautilus_trader.serialization.arrow.serializer import list_schemas
from nautilus_trader.serialization.arrow.serializer import register_parquet
from nautilus_trader.serialization.arrow.util import GENERIC_DATA_PREFIX
from nautilus_trader.serialization.arrow.util import list_dicts_to_dict_lists


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
        fs_protocol: str = "file",
        flush_interval_ms: Optional[int] = None,
        replace: bool = False,
        include_types: Optional[Tuple[type]] = None,
    ):
        self.fs: fsspec.AbstractFileSystem = fsspec.filesystem(fs_protocol)
        self.path = self._check_path(path)
        self.include_types = include_types
        if self.fs.exists(self.path) and replace:
            for fn in self.fs.ls(self.path):
                self.fs.rm(fn)
            self.fs.rmdir(self.path)
        self.fs.mkdir(self.path)
        self._schemas = list_schemas()
        self._schemas.update(
            {
                OrderBookDelta: self._schemas[OrderBookData],
                OrderBookDeltas: self._schemas[OrderBookData],
                OrderBookSnapshot: self._schemas[OrderBookData],
            }
        )
        self.logger = logger
        self._files: Dict[type, BinaryIO] = {}
        self._writers: Dict[type, RecordBatchStreamWriter] = {}
        self._create_writers()
        self.flush_interval_ms = datetime.timedelta(milliseconds=flush_interval_ms or 1000)
        self._last_flush = datetime.datetime(1970, 1, 1)  # Default value to begin
        self.missing_writers: Set[type] = set()

    def _check_path(self, p: str) -> str:
        path = pathlib.Path(p)
        err_parent = f"Parent of path {path} does not exist, please create it"
        assert self.fs.exists(resolve_path(path.parent, fs=self.fs)), err_parent
        err_dir_empty = "Path must be directory or empty"
        str_path = resolve_path(path, fs=self.fs)
        assert self.fs.isdir(str_path) or not self.fs.exists(str_path), err_dir_empty
        return str_path

    def _create_writers(self):
        for cls in self._schemas:
            if self.include_types is not None and cls.__name__ not in self.include_types:
                continue
            table_name = get_cls_table(cls).__name__
            if table_name in self._writers:
                continue
            prefix = GENERIC_DATA_PREFIX if not is_nautilus_class(cls) else ""
            schema = self._schemas[cls]
            full_path = f"{self.path}/{prefix}{table_name}.feather"
            f = self.fs.open(str(full_path), "wb")
            self._files[cls] = f
            self._writers[table_name] = pa.ipc.new_stream(f, schema)

    def handle_signal(self, signal: Data):
        def serialize(self):
            return {
                "ts_init": self.ts_init,
                "value": self.value,
            }

        register_parquet(cls=type(signal), serializer=serialize)

        schema = pa.schema(
            {
                "ts_init": pa.uint64(),
                "value": {int: pa.int64(), float: pa.float64(), str: pa.string()}[
                    type(signal.value)
                ],
            }
        )
        # Refresh schemas, create writer for new table
        cls = type(signal)
        self._schemas[cls] = schema
        table_name = get_cls_table(cls).__name__
        schema = self._schemas[cls]
        full_path = f"{self.path}/{table_name}.feather"
        f = self.fs.open(str(full_path), "wb")
        self._files[cls] = f
        self._writers[table_name] = pa.ipc.new_stream(f, schema)

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
        table = get_cls_table(cls).__name__
        if table not in self._writers:
            if table.startswith("Signal"):
                self.handle_signal(obj)
            elif cls not in self.missing_writers:
                self.logger.warning(f"Can't find writer for cls: {cls}")
                self.missing_writers.add(cls)
                return
            else:
                return
        writer: RecordBatchStreamWriter = self._writers[table]
        serialized = ParquetSerializer.serialize(obj)
        if isinstance(serialized, dict):
            serialized = [serialized]
        original = list_dicts_to_dict_lists(
            serialized,
            keys=self._schemas[cls].names,
        )
        data = list(original.values())
        try:
            batch = pa.record_batch(data, schema=self._schemas[cls])
            writer.write_batch(batch)
            self.check_flush()
        except Exception as e:
            self.logger.error(f"Failed to serialize {cls=}")
            self.logger.error(f"ERROR = `{e}`")
            self.logger.debug(f"data = {original}")

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
        for cls in self._files:
            self._files[cls].flush()

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


def generate_signal_class(name: str):
    """
    Dynamically create a Data subclass for this signal.
    """

    class SignalData(Data):
        """
        Represents generic signal data.
        """

        def __init__(self, value, ts_event: int, ts_init: int):
            super().__init__(ts_event=ts_event, ts_init=ts_init)
            self.value = value

    SignalData.__name__ = f"Signal{name.title()}"
    return SignalData
