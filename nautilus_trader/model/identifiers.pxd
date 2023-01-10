# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.rust.model cimport AccountId_t
from nautilus_trader.core.rust.model cimport ClientOrderId_t
from nautilus_trader.core.rust.model cimport ComponentId_t
from nautilus_trader.core.rust.model cimport InstrumentId_t
from nautilus_trader.core.rust.model cimport OrderListId_t
from nautilus_trader.core.rust.model cimport PositionId_t
from nautilus_trader.core.rust.model cimport Symbol_t
from nautilus_trader.core.rust.model cimport TradeId_t
from nautilus_trader.core.rust.model cimport Venue_t
from nautilus_trader.core.rust.model cimport VenueOrderId_t


cdef class Identifier:
    cdef str to_str(self)


cdef class Symbol(Identifier):
    cdef Symbol_t _mem


cdef class Venue(Identifier):
    cdef Venue_t _mem


cdef class InstrumentId(Identifier):
    cdef InstrumentId_t _mem

    cdef readonly Symbol symbol
    """The instrument ticker symbol.\n\n:returns: `Symbol`"""
    cdef readonly Venue venue
    """The instrument trading venue.\n\n:returns: `Venue`"""

    @staticmethod
    cdef InstrumentId from_mem_c(InstrumentId_t mem)

    @staticmethod
    cdef InstrumentId from_str_c(str value)


cdef class ComponentId(Identifier):
    cdef ComponentId_t _mem


cdef class ClientId(ComponentId):
    pass


cdef class TraderId(ComponentId):
    cpdef str get_tag(self)


cdef class StrategyId(ComponentId):
    cpdef str get_tag(self)
    cpdef bint is_external(self)
    @staticmethod
    cdef StrategyId external_c()


cdef class ExecAlgorithmId(ComponentId):
    pass


cdef class AccountId(Identifier):
    cdef AccountId_t _mem

    cpdef str get_issuer(self)
    cpdef str get_id(self)


cdef class ClientOrderId(Identifier):
    cdef ClientOrderId_t _mem


cdef class VenueOrderId(Identifier):
    cdef VenueOrderId_t _mem


cdef class OrderListId(Identifier):
    cdef OrderListId_t _mem


cdef class PositionId(Identifier):
    cdef PositionId_t _mem

    @staticmethod
    cdef PositionId from_mem_c(PositionId_t mem)
    cdef bint is_virtual_c(self) except *


cdef class TradeId(Identifier):
    cdef TradeId_t _mem

    @staticmethod
    cdef TradeId from_mem_c(TradeId_t mem)
