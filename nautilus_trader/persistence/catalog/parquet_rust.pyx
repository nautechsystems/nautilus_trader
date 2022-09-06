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

TODO(cs): Implement
from libc.stdint cimport uint64_t

from nautilus_trader.core.rust.model cimport Bar_t
from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.core.rust.persistence cimport Vec_Bar
from nautilus_trader.core.rust.persistence cimport Vec_QuoteTick
from nautilus_trader.core.rust.persistence cimport index_bar_vector
from nautilus_trader.core.rust.persistence cimport index_quote_tick_vector
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.tick cimport QuoteTick


cdef list parse_quote_tick_vector(Vec_QuoteTick tick_vec):
    cdef list ticks = []

    cdef:
        QuoteTick_t _mem
        QuoteTick tick
        uint64_t i
    for i in range(0, tick_vec.len - 1):
        tick = QuoteTick.__new__(QuoteTick)
        tick.ts_event = _mem.ts_event
        tick.ts_init = _mem.ts_init
        tick._mem = index_quote_tick_vector(&tick_vec, i)[0]
        ticks.append(tick)

    return ticks


cdef list parse_bar_vector(Vec_Bar bar_vec):
    cdef list bars = []

    cdef:
        Bar_t _mem
        Bar bar
        uint64_t i
    for i in range(0, bar_vec.len - 1):
        bar = Bar.__new__(Bar)
        bar.ts_event = _mem.ts_event
        bar.ts_init = _mem.ts_init
        bar._mem = index_bar_vector(&bar_vec, i)[0]
        bars.append(bar)

    return bars
