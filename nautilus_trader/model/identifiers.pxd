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

from nautilus_trader.core.rust.model cimport AccountId_t
from nautilus_trader.core.rust.model cimport ClientId_t
from nautilus_trader.core.rust.model cimport ClientOrderId_t
from nautilus_trader.core.rust.model cimport ComponentId_t
from nautilus_trader.core.rust.model cimport ExecAlgorithmId_t
from nautilus_trader.core.rust.model cimport InstrumentId_t
from nautilus_trader.core.rust.model cimport OrderListId_t
from nautilus_trader.core.rust.model cimport PositionId_t
from nautilus_trader.core.rust.model cimport StrategyId_t
from nautilus_trader.core.rust.model cimport Symbol_t
from nautilus_trader.core.rust.model cimport TradeId_t
from nautilus_trader.core.rust.model cimport TraderId_t
from nautilus_trader.core.rust.model cimport Venue_t
from nautilus_trader.core.rust.model cimport VenueOrderId_t


cdef class Identifier:
    cdef str to_str(self)


cdef class Symbol(Identifier):
    cdef Symbol_t _mem

    @staticmethod
    cdef Symbol from_mem_c(Symbol_t mem)
    cpdef bint is_composite(self)
    cpdef str root(self)
    cpdef str topic(self)


cdef class Venue(Identifier):
    cdef Venue_t _mem

    @staticmethod
    cdef Venue from_mem_c(Venue_t mem)
    @staticmethod
    cdef Venue from_code_c(str code)
    cpdef bint is_synthetic(self)


cdef class InstrumentId(Identifier):
    cdef InstrumentId_t _mem

    @staticmethod
    cdef InstrumentId from_mem_c(InstrumentId_t mem)
    @staticmethod
    cdef InstrumentId from_str_c(str value)
    cpdef bint is_synthetic(self)
    cpdef object to_pyo3(self)


cdef class ComponentId(Identifier):
    cdef ComponentId_t _mem

    @staticmethod
    cdef ComponentId from_mem_c(ComponentId_t mem)


cdef class ClientId(Identifier):
    cdef ClientId_t _mem

    @staticmethod
    cdef ClientId from_mem_c(ClientId_t mem)


cdef class TraderId(Identifier):
    cdef TraderId_t _mem

    @staticmethod
    cdef TraderId from_mem_c(TraderId_t mem)

    cpdef str get_tag(self)


cdef class StrategyId(Identifier):
    cdef StrategyId_t _mem

    @staticmethod
    cdef StrategyId from_mem_c(StrategyId_t mem)
    @staticmethod
    cdef StrategyId external_c()
    cpdef str get_tag(self)
    cpdef bint is_external(self)


cdef class ExecAlgorithmId(Identifier):
    cdef ExecAlgorithmId_t _mem

    @staticmethod
    cdef ExecAlgorithmId from_mem_c(ExecAlgorithmId_t mem)


cdef class AccountId(Identifier):
    cdef AccountId_t _mem

    @staticmethod
    cdef AccountId from_mem_c(AccountId_t mem)
    cpdef str get_issuer(self)
    cpdef str get_id(self)


cdef class ClientOrderId(Identifier):
    cdef ClientOrderId_t _mem

    @staticmethod
    cdef ClientOrderId from_mem_c(ClientOrderId_t mem)


cdef class VenueOrderId(Identifier):
    cdef VenueOrderId_t _mem

    @staticmethod
    cdef VenueOrderId from_mem_c(VenueOrderId_t mem)


cdef class OrderListId(Identifier):
    cdef OrderListId_t _mem

    @staticmethod
    cdef OrderListId from_mem_c(OrderListId_t mem)


cdef class PositionId(Identifier):
    cdef PositionId_t _mem

    @staticmethod
    cdef PositionId from_mem_c(PositionId_t mem)
    cdef bint is_virtual_c(self)


cdef class TradeId(Identifier):
    cdef TradeId_t _mem

    @staticmethod
    cdef TradeId from_mem_c(TradeId_t mem)
