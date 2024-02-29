# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
from io import TextIOWrapper
from typing import Any, BinaryIO

import fsspec
import pyarrow as pa
from fsspec.compression import AbstractBufferedFile
from pyarrow import RecordBatchStreamWriter

from nautilus_trader.common.component import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.persistence.funcs import class_to_filename
from nautilus_trader.persistence.funcs import urisafe_instrument_id
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
    fs_protocol : str, default 'file'
        The `fsspec` file system protocol.
    flush_interval_ms : int, optional
        The flush interval (milliseconds) for writing chunks.
    replace : bool, default False
        If existing files at the given `path` should be replaced.
    include_types : list[type], optional
        A list of Arrow serializable types to write.
        If this is specified then *only* the included types will be written.

    """

    def __init__(
        self,
        path: str,
        fs_protocol: str | None = "file",
        flush_interval_ms: int | None = None,
        replace: bool = False,
        include_types: list[type] | None = None,
    ) -> None:
        self.path = path
        self.fs: fsspec.AbstractFileSystem = fsspec.filesystem(fs_protocol)
        self.fs.makedirs(self.fs._parent(self.path), exist_ok=True)

        if self.fs.exists(self.path) and not self.fs.isdir(self.path):
            raise FileNotFoundError("Path must be directory or empty")

        self.include_types = include_types
        if self.fs.exists(self.path) and replace:
            for fn in self.fs.ls(self.path):
                self.fs.rm(fn)
            self.fs.rmdir(self.path)

        self.fs.makedirs(self.path, exist_ok=True)

        self._schemas = list_schemas()
        self.logger = Logger(type(self).__name__)
        self._files: dict[object, TextIOWrapper | BinaryIO | AbstractBufferedFile] = {}
        self._writers: dict[str, RecordBatchStreamWriter] = {}
        self._instrument_writers: dict[tuple[str, str], RecordBatchStreamWriter] = {}
        self._per_instrument_writers = {
            "trade_tick",
            "quote_tick",
            "order_book_delta",
            "ticker",
        }
        self._instruments: dict[InstrumentId, Instrument] = {}
        self._create_writers()

        self.flush_interval_ms = datetime.timedelta(milliseconds=flush_interval_ms or 1000)
        self._last_flush = datetime.datetime(1970, 1, 1)  # Default value to begin
        self.missing_writers: set[type] = set()

    @property
    def is_closed(self) -> bool:
        """
        Return whether all file streams are closed.

        Returns
        -------
        bool

        """
        return all(self._files[table_name].closed for table_name in self._files)

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
        full_path = f"{self.path}/{table_name}.feather"

        self.fs.makedirs(self.fs._parent(full_path), exist_ok=True)
        f = self.fs.open(full_path, "wb")
        self._files[table_name] = f
        self._writers[table_name] = pa.ipc.new_stream(f, schema)

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

        full_path = f"{folder}/{urisafe_instrument_id(obj.instrument_id.value)}.feather"
        f = self.fs.open(full_path, "wb")
        self._files[key] = f
        self._instrument_writers[key] = pa.ipc.new_stream(f, schema)

    def _extract_obj_metadata(
        self,
        obj: TradeTick | QuoteTick | Bar | OrderBookDelta,
    ) -> dict[bytes, bytes]:
        instrument = self._instruments[obj.instrument_id]
        metadata = {b"instrument_id": obj.instrument_id.value.encode()}
        if isinstance(obj, OrderBookDelta):
            metadata.update(
                {
                    b"price_precision": str(instrument.price_precision).encode(),
                    b"size_precision": str(instrument.size_precision).encode(),
                },
            )
        elif isinstance(obj, OrderBookDeltas):
            metadata.update(
                {
                    b"price_precision": str(instrument.price_precision).encode(),
                    b"size_precision": str(instrument.size_precision).encode(),
                },
            )
        elif isinstance(obj, QuoteTick | TradeTick):
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
        if isinstance(obj, CustomData):
            cls = obj.data_type.type
        elif isinstance(obj, Instrument):
            if obj.id not in self._instruments:
                self._instruments[obj.id] = obj

        table = class_to_filename(cls)
        if isinstance(obj, Bar):
            bar: Bar = obj
            table += f"_{str(bar.bar_type).lower()}"

        if table not in self._writers:
            if table.startswith("custom_signal"):
                self._create_writer(cls=cls)
            elif table.startswith("bar"):
                self._create_writer(cls=cls, table_name=table)
            elif table in self._per_instrument_writers:
                key = (table, obj.instrument_id.value)  # type: ignore
                if key not in self._instrument_writers:
                    self._create_instrument_writer(cls=cls, obj=obj)
            elif cls not in self.missing_writers:
                self.logger.warning(f"Can't find writer for cls: {cls}")
                self.missing_writers.add(cls)
                return
            else:
                return
        if table in self._per_instrument_writers:
            writer: RecordBatchStreamWriter = self._instrument_writers[(table, obj.instrument_id.value)]  # type: ignore
        else:
            writer: RecordBatchStreamWriter = self._writers[table]  # type: ignore
        serialized = ArrowSerializer.serialize_batch([obj], data_cls=cls)
        if not serialized:
            return
        try:
            writer.write_table(serialized)
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
        for wcls in tuple(self._writers):
            self._writers[wcls].close()
            del self._writers[wcls]
        for fcls in self._files:
            self._files[fcls].close()


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
    def serialize_signal(data: SignalData) -> pa.RecordBatch:
        return pa.RecordBatch.from_pylist(
            [
                {
                    "ts_init": data.ts_init,
                    "ts_event": data.ts_event,
                    "value": data.value,
                },
            ],
            schema=schema,
        )

    def deserialize_signal(table: pa.Table) -> list[SignalData]:
        return [SignalData(**d) for d in table.to_pylist()]

    schema = pa.schema(
        {
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
            "value": {int: pa.int64(), float: pa.float64(), str: pa.string()}[value_type],
        },
    )
    register_arrow(
        data_cls=SignalData,
        encoder=serialize_signal,
        decoder=deserialize_signal,
        schema=schema,
    )

    return SignalData
