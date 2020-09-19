# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef class Indicator:
    cdef readonly str name
    cdef readonly str params
    cdef readonly bint has_inputs
    cdef readonly bint initialized

    cpdef void handle_quote_tick(self, QuoteTick tick) except *
    cpdef void handle_trade_tick(self, TradeTick tick) except *
    cpdef void handle_bar(self, Bar bar) except *
    cpdef void reset(self) except *

    cdef inline Price _get_quote_price(self, QuoteTick tick, PriceType price_type)
    cdef void _set_has_inputs(self, bint setting) except *
    cdef void _set_initialized(self, bint setting) except *
    cdef void _reset_base(self) except *
