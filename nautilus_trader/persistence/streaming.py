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

import datetime
import pathlib
from typing import BinaryIO, Dict

import fsspec
import pyarrow as pa
from pyarrow import RecordBatchStreamWriter

from nautilus_trader.core.inspect import is_nautilus_class
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.serialization.arrow.serializer import get_cls_table
from nautilus_trader.serialization.arrow.serializer import list_schemas
from nautilus_trader.serialization.arrow.util import list_dicts_to_dict_lists


class FeatherWriter:
    """
    Provides a stream writer of Nautilus objects into feather files.
    """

    def __init__(self, path: str, fs_protocol: str = "file", flush_interval=None, replace=False):
        """
        Initialize a new instance of the ``FeatherWriter`` class.
        """
        self.fs: fsspec.AbstractFileSystem = fsspec.filesystem(fs_protocol)
        self.path = str(self._check_path(path))
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
        self._files: Dict[type, BinaryIO] = {}
        self._writers: Dict[type, RecordBatchStreamWriter] = {}
        self._create_writers()
        self.flush_interval = flush_interval or datetime.timedelta(milliseconds=1000)
        self._last_flush = datetime.datetime(1970, 1, 1)

    def _check_path(self, p):
        path = pathlib.Path(p)
        err_parent = f"Parent of path {path} does not exist, please create it"
        assert self.fs.exists(str(path.parent)), err_parent
        err_dir_empty = "Path must be directory or empty"
        assert self.fs.isdir(str(path)) or not self.fs.exists(str(path)), err_dir_empty
        return path

    def _create_writers(self):
        for cls in self._schemas:
            table_name = get_cls_table(cls).__name__
            if table_name in self._writers:
                continue
            prefix = "genericdata_" if not is_nautilus_class(cls) else ""
            schema = self._schemas[cls]
            full_path = f"{self.path}/{prefix}{table_name}.feather"
            f = self.fs.open(str(full_path), "wb")
            self._files[cls] = f
            self._writers[table_name] = pa.ipc.new_stream(f, schema)

    def write(self, obj: object):
        assert obj is not None
        cls = obj.__class__
        if isinstance(obj, GenericData):
            cls = obj.data_type.type
        table = get_cls_table(cls).__name__
        if table not in self._writers:
            print(f"Can't find writer for cls: {cls}")
            return
        writer = self._writers[table]
        serialized = ParquetSerializer.serialize(obj)
        if isinstance(serialized, dict):
            serialized = [serialized]
        data = list_dicts_to_dict_lists(
            serialized,
            keys=self._schemas[cls].names,
        )
        data = list(data.values())
        batch = pa.record_batch(data, schema=self._schemas[cls])
        writer.write_batch(batch)
        self.check_flush()

    def check_flush(self):
        now = datetime.datetime.now()
        if now - self._last_flush > self.flush_interval:
            self.flush()
            self._last_flush = now

    def flush(self):
        for cls in self._files:
            self._files[cls].flush()

    def close(self):
        self.flush()
        for cls in self._writers:
            self._writers[cls].close()


def read_feather(path: str, fs: fsspec.AbstractFileSystem = None):
    fs = fs or fsspec.filesystem("file")
    if not fs.exists(path):
        return
    try:
        with fs.open(path) as f:
            reader = pa.ipc.open_stream(f)
            return reader.read_pandas()
    except (pa.ArrowInvalid, FileNotFoundError):
        return
