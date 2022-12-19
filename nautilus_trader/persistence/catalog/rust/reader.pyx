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
from nautilus_trader.core.rust.model cimport quote_tick_copy
from nautilus_trader.core.rust.model cimport trade_tick_copy
from nautilus_trader.core.rust.persistence cimport ParquetReaderType
from nautilus_trader.core.rust.persistence cimport ParquetType
from nautilus_trader.core.rust.persistence cimport parquet_reader_drop_chunk
from nautilus_trader.core.rust.persistence cimport parquet_reader_file_new
from nautilus_trader.core.rust.persistence cimport parquet_reader_free
from nautilus_trader.core.rust.persistence cimport parquet_reader_next_chunk
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick

from nautilus_trader.core.rust.model cimport quote_tick_copy
from nautilus_trader.core.rust.model cimport trade_tick_copy

cdef class ParquetReader:
    """
    Provides a parquet file reader implemented in Rust under the hood.
    """

    def __del__(self) -> None:
        parquet_reader_free(
            reader=self._reader,
            parquet_type=self._parquet_type,
            reader_type=self._reader_type,
        )
        self._drop_chunk()

    def __iter__(self):
        while True:
            self._chunk = parquet_reader_next_chunk(
                reader=self._reader,
                parquet_type=self._parquet_type,
                reader_type=self._reader_type,
            )

            if self._chunk.len == 0:
                self._drop_chunk()
                return # Stop iterating

            # Initialize Python objects from the rust vector
            if self._parquet_type == ParquetType.QuoteTick:
                yield _parse_quote_tick_chunk(self._chunk)
            elif self._parquet_type == ParquetType.TradeTick:
                yield _parse_trade_tick_chunk(self._chunk)
            else:
                raise NotImplementedError("")

            self._drop_chunk()

    cdef void _drop_chunk(self) except *:
        # TODO(cs): Added this for safety although doesn't seem to make a difference
        if self._chunk.ptr == NULL:
            return
        # Drop the previous chunk
        parquet_reader_drop_chunk(self._chunk, self._parquet_type)


cdef class ParquetFileReader(ParquetReader):
    """
    Provides a parquet reader implemented in Rust under the hood.
    """

    def __init__(
        self,
        type parquet_type,
        str file_path,
        uint64_t chunk_size=1000,  # TBD
    ):
        Condition.valid_string(file_path, "file_path")
        if not os.path.exists(file_path):
            raise FileNotFoundError(f"File not found at {file_path}")

        self._file_path = file_path
        self._parquet_type = py_type_to_parquet_type(parquet_type)
        self._reader_type = ParquetReaderType.File
        self._reader = parquet_reader_file_new(
            file_path=<PyObject *>self._file_path,
            parquet_type=self._parquet_type,
            chunk_size=chunk_size,
        )


# cdef class ParquetBufferReader(ParquetReader):
#     """
#     Provides a parquet buffer reader implemented in Rust under the hood.
#     """
#
#     def __init__(
#         self,
#         type parquet_type,
#         uint64_t chunk_size=1000,  # TBD
#     ):
#         self._parquet_type = py_type_to_parquet_type(parquet_type)
#         cdef void *reader = parquet_reader_buffer_new(
#             data=**NEEDS A CVec**
#             parquet_type=self._parquet_type,
#             chunk_size=chunk_size,
#         )


cdef inline list _parse_quote_tick_chunk(CVec chunk):
    cdef:
        QuoteTick_t _mem
        QuoteTick obj
        list objs = []
        uint64_t i
    for i in range(0, chunk.len):
        obj = QuoteTick.__new__(QuoteTick)
        _mem = (<QuoteTick_t *>chunk.ptr)[i]
        obj.ts_init = _mem.ts_init
        obj.ts_event = _mem.ts_event
        obj._mem = quote_tick_copy(&_mem)
        objs.append(obj)

    return objs

cdef inline list _parse_trade_tick_chunk(CVec chunk):
    cdef:
        TradeTick_t _mem
        TradeTick obj
        list objs = []
        uint64_t i
    for i in range(0, chunk.len):
        obj = TradeTick.__new__(TradeTick)
        _mem = (<TradeTick_t *>chunk.ptr)[i]
        obj.ts_init = _mem.ts_init
        obj.ts_event = _mem.ts_event
        obj._mem = trade_tick_copy(&_mem)
        objs.append(obj)

    return objs
