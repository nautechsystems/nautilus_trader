# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick


cdef class Indicator:
    cdef list _params

    cdef readonly str name
    """The name of the indicator.\n\n:returns: `str`"""
    cdef readonly bint has_inputs
    """If the indicator has received inputs.\n\n:returns: `bool`"""
    cdef readonly bint initialized
    """If the indicator is warmed up and initialized.\n\n:returns: `bool`"""

    cdef str _params_str(self)

    cpdef void handle_quote_tick(self, QuoteTick tick)
    cpdef void handle_trade_tick(self, TradeTick tick)
    cpdef void handle_bar(self, Bar bar)
    cpdef void reset(self)

    cpdef void _set_has_inputs(self, bint setting)
    cpdef void _set_initialized(self, bint setting)
    cpdef void _reset(self)
