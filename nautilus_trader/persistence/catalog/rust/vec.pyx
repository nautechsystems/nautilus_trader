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

# distutils: language = c++
from libcpp.vector cimport vector

from nautilus_trader.core.rust.model cimport QuoteTick_t
from nautilus_trader.core.rust.model cimport TradeTick_t
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick


cdef inline void* create_vector(list items):
    if isinstance(items[0], QuoteTick):
        return _create_quote_tick_vector(items)
    elif isinstance(items[0], TradeTick):
        return _create_trade_tick_vector(items)


cdef inline void* _create_quote_tick_vector(list items):
    cdef vector[QuoteTick_t] vec
    [vec.push_back(<QuoteTick_t>(<QuoteTick>item)._mem) for item in items]
    return <void*>vec.data()


cdef inline void* _create_trade_tick_vector(list items):
    cdef vector[TradeTick_t] vec
    [vec.push_back(<TradeTick_t>(<TradeTick>item)._mem) for item in items]
    return <void*>vec.data()
