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

from cpython.datetime cimport datetime

from nautilus_trader.model.c_enums.maker cimport Maker
from nautilus_trader.model.identifiers cimport MatchId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Tick:
    cdef readonly Symbol symbol
    cdef readonly datetime timestamp

    cpdef bint equals(self, Tick other)
    cpdef str to_string(self)
    cpdef str to_serializable_string(self)


cdef class QuoteTick(Tick):
    cdef readonly Price bid
    cdef readonly Price ask
    cdef readonly Quantity bid_size
    cdef readonly Quantity ask_size

    @staticmethod
    cdef QuoteTick from_serializable_string(Symbol symbol, str values)


cdef class TradeTick(Tick):
    cdef readonly Price price
    cdef readonly Quantity size
    cdef readonly Maker maker
    cdef readonly MatchId match_id

    @staticmethod
    cdef TradeTick from_serializable_string(Symbol symbol, str values)
