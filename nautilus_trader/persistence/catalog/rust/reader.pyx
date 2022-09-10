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

import os

from nautilus_trader.persistence.catalog.rust.common import py_type_to_parquet_type

from cpython.object cimport PyObject
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.core.rust.model cimport TradeTick_t
from nautilus_trader.core.rust.persistence cimport ParquetType
from nautilus_trader.core.rust.persistence cimport parquet_reader_drop
from nautilus_trader.core.rust.persistence cimport parquet_reader_drop_chunk
from nautilus_trader.core.rust.persistence cimport parquet_reader_index_chunk
from nautilus_trader.core.rust.persistence cimport parquet_reader_new
from nautilus_trader.core.rust.persistence cimport parquet_reader_next_chunk
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick


cdef class ParquetReader:
    """
    Provides a parquet reader implemented in Rust under the hood.
    """

    def __init__(
        self,
        str file_path,
        type parquet_type,
        uint64_t chunk_size=1000,  # TBD
    ):
        Condition.valid_string(file_path, "file_path")
        if not os.path.exists(file_path):
            raise FileNotFoundError(f"File not found at {file_path}")

        self._file_path = file_path
        self._parquet_type = py_type_to_parquet_type(parquet_type)
        self._reader = parquet_reader_new(
            file_path=<PyObject *>self._file_path,
            reader_type=self._parquet_type,
            chunk_size=chunk_size,
        )

    def __del__(self) -> None:
        parquet_reader_drop(self._reader, self._parquet_type)
        self._drop_chunk()

    def __iter__(self):
        cdef list chunk = self._next_chunk()
        while chunk:
            yield chunk
            chunk = self._next_chunk()

    cdef list _next_chunk(self):
        self._drop_chunk()
        self._chunk = parquet_reader_next_chunk(self._reader, self._parquet_type)

        if self._chunk.len == 0:
            return None # stop iteration

        return self._parse_chunk(self._chunk)

    cdef list _parse_chunk(self, CVec chunk):
        # Initialize Python objects from the rust vector.
        if self._parquet_type == ParquetType.QuoteTick:
            return _parse_quote_tick_chunk(chunk)
        elif self._parquet_type == ParquetType.TradeTick:
            return _parse_trade_tick_chunk(chunk)
        else:
            raise RuntimeError("")

    cdef void _drop_chunk(self) except *:
        # Drop the previous chunk
        parquet_reader_drop_chunk(self._chunk, self._parquet_type)


cdef list _parse_quote_tick_chunk(CVec chunk):
    cdef list ticks = []

    cdef:
        QuoteTick_t _mem
        QuoteTick tick
        uint64_t i
    for i in range(0, chunk.len):
        _mem = (<QuoteTick_t *>(parquet_reader_index_chunk(chunk, ParquetType.QuoteTick, i)))[0]
        tick = QuoteTick.__new__(QuoteTick)
        tick.ts_event = _mem.ts_event
        tick.ts_init = _mem.ts_init
        tick._mem = _mem
        ticks.append(tick)

    return ticks

cdef list _parse_trade_tick_chunk(CVec chunk):
    cdef list ticks = []

    cdef:
        TradeTick_t _mem
        TradeTick tick
        uint64_t i
    for i in range(0, chunk.len):
        _mem = (<TradeTick_t *>(parquet_reader_index_chunk(chunk, ParquetType.TradeTick, i)))[0]
        tick = TradeTick.__new__(TradeTick)
        tick.ts_event = _mem.ts_event
        tick.ts_init = _mem.ts_init
        tick._mem = _mem
        ticks.append(tick)

    return ticks
