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

from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.core.rust.catalog cimport Vec_QuoteTick
from nautilus_trader.core.rust.catalog cimport quote_tick_reader_new
from nautilus_trader.core.rust.catalog cimport quote_tick_reader_next
from nautilus_trader.core.rust.catalog cimport quote_tick_vector_index


cdef class QuoteTickReader(ParquetReader):
    def __init__(self, file_name, batch_size):
        self.chunk_iterator = quote_tick_reader_new(file_name, batch_size)

    cdef list next(self):
        cdef Vec_QuoteTick tick_vec = quote_tick_reader_next(self.chunk_iterator)
        cdef list ticks = self._initialize(tick_vec) # List[QuoteTick]
        return ticks
        
    cdef list _initialize(Vec_QuoteTick tick_vec):
        cdef QuoteTick_t _mem
        cdef QuoteTick tick
        cdef list ticks = []
        for i in range(0, tick_vec.len - 1):
            tick = QuoteTick.__new__(QuoteTick)
            tick.ts_event = _mem.ts_event
            tick.ts_init = _mem.ts_init
            tick._mem = quote_tick_vector_index(&tick_vec, i)[0]
            ticks.append(tick)
        return ticks
