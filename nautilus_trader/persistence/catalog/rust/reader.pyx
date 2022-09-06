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
from cpython.object cimport PyObject
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t
from libc.stdint cimport uintptr_t

from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.core.rust.persistence cimport ParquetType
from nautilus_trader.core.rust.persistence cimport parquet_reader_drop
from nautilus_trader.core.rust.persistence cimport parquet_reader_drop_chunk
from nautilus_trader.core.rust.persistence cimport parquet_reader_index_chunk
from nautilus_trader.core.rust.persistence cimport parquet_reader_new
from nautilus_trader.core.rust.persistence cimport parquet_reader_next_chunk
from nautilus_trader.model.data.tick cimport QuoteTick


def py_type_to_parquet_type(cls: type):
    if cls == QuoteTick:
        return ParquetType.QuoteTick
    else:
        raise RuntimeError(f"Type {cls} not supported as a ParquetType yet.")

cdef _parse_quote_tick_chunk(CVec chunk):
    cdef QuoteTick_t _mem
    cdef QuoteTick tick
    cdef list ticks = []
    for i in range(0, chunk.len):
        _mem = (<QuoteTick_t *>(parquet_reader_index_chunk(chunk, ParquetType.QuoteTick, i)))[0]
        tick = QuoteTick.__new__(QuoteTick)
        tick.ts_event = _mem.ts_event
        tick.ts_init = _mem.ts_init
        tick._mem = _mem
        ticks.append(tick)
    return ticks

cdef class ParquetReader:
    def __init__(self, file_path, parquet_type: type):
        self.file_path = file_path
        self.parquet_type = py_type_to_parquet_type(parquet_type)
        # TODO Check if file exists
        # TODO Check parquet_type is valid type
        self.reader = parquet_reader_new(<PyObject *>self.file_path, self.parquet_type)

    def __iter__(self):
        chunk = self._next_chunk()
        while chunk:
            yield chunk
            chunk = self._next_chunk()

    cpdef list _next_chunk(self):
        self._drop_chunk()
        self.chunk = parquet_reader_next_chunk(self.reader, self.parquet_type)

        if self.chunk.len == 0:
            return None # stop iteration

        return self._parse_chunk(self.chunk)

    cdef list _parse_chunk(self, CVec chunk):
        # Initialize Python objects from the rust vector.
        if self.parquet_type == ParquetType.QuoteTick:
            return _parse_quote_tick_chunk(chunk)
        else:
            raise RuntimeError("")

    def __del__(self) -> None:
        self._drop()

    cpdef void _drop(self):
        parquet_reader_drop(self.reader, self.parquet_type)
        self._drop_chunk()

    cpdef void _drop_chunk(self):
        # Drop the previous chunk
        parquet_reader_drop_chunk(self.chunk, self.parquet_type)

