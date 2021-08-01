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

from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.serialization.arrow.core import _schemas
from nautilus_trader.serialization.arrow.core import _serialize
from nautilus_trader.serialization.arrow.util import list_dicts_to_dict_lists


class ArrowWriter:
    def __init__(self, directory: str, fs_type="file", flush_interval=None):
        self.path = pathlib.Path(directory)
        assert self.path.parent.exists()
        assert self.path.is_dir() or not self.path.exists(), "Path must be directory or empty"
        self.path.mkdir(exist_ok=True)
        self.fs = fsspec.filesystem(fs_type)
        self._schemas = _schemas
        self._schemas.update(
            {
                OrderBookDelta: self._schemas[OrderBookData],
                OrderBookDeltas: self._schemas[OrderBookData],
                OrderBookSnapshot: self._schemas[OrderBookData],
            }
        )
        self._ignore_keys = {
            OrderBookDelta: (
                "_last",
                "type",
            ),
            OrderBookDeltas: (
                "_last",
                "type",
            ),
            OrderBookSnapshot: (
                "_last",
                "type",
            ),
        }
        self._files: Dict[type, BinaryIO] = {}
        self._writers: Dict[type, RecordBatchStreamWriter] = {}
        self._create_writers()
        self.flush_interval = flush_interval or datetime.timedelta(milliseconds=1000)
        self._last_flush = datetime.datetime(1970, 1, 1)

    @staticmethod
    def is_nautilus_builtin(cls):
        return cls.__module__.startswith("nautilus_trader.model.")

    def _create_writers(self):
        for cls in self._schemas:
            prefix = "genericdata_" if not self.is_nautilus_builtin(cls) else ""
            schema = self._schemas[cls]
            f = self.fs.open(str(self.path.joinpath(f"{prefix}{cls.__name__}.feather")), "wb")
            self._files[cls] = f
            self._writers[cls] = pa.ipc.new_stream(f, schema)

    def write(self, obj: object):
        assert obj is not None
        cls = obj.__class__
        if isinstance(obj, GenericData):
            cls = obj.data_type.type
        if cls not in self._writers:
            print(f"Can't find writer for cls: {cls}")
            return
        writer = self._writers[cls]
        serialized = _serialize(obj)
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
