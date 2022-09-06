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
from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.core.rust.persistence cimport ParquetType
from nautilus_trader.core.rust.persistence cimport parquet_reader_new
from nautilus_trader.core.rust.persistence cimport parquet_reader_next_chunk
from nautilus_trader.model.data.tick cimport QuoteTick


cpdef list read_parquet_quote_ticks(str file_path):
    cdef list ticks = []

    reader = parquet_reader_new(<PyObject *>file_path, ParquetType.QuoteTick)
    cdef CVec quotes_vec = parquet_reader_next_chunk(reader, ParquetType.QuoteTick)
    cdef QuoteTick_t *ptr = <QuoteTick_t *>quotes_vec.ptr

    cdef:
        QuoteTick_t rust_tick
        QuoteTick tick
        uint64_t i
    for i in range(0, quotes_vec.len - 1):
        rust_tick = ptr[i]

        tick = QuoteTick.__new__(QuoteTick)
        tick.ts_event = rust_tick.ts_event
        tick.ts_init = rust_tick.ts_init
        tick._mem = rust_tick

        ticks.append(tick)

    return ticks
