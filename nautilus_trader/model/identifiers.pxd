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

from nautilus_trader.core.rust.model cimport AccountId_t
from nautilus_trader.core.rust.model cimport ClientOrderId_t
from nautilus_trader.core.rust.model cimport ClientOrderLinkId_t
from nautilus_trader.core.rust.model cimport ComponentId_t
from nautilus_trader.core.rust.model cimport InstrumentId_t
from nautilus_trader.core.rust.model cimport OrderListId_t
from nautilus_trader.core.rust.model cimport PositionId_t
from nautilus_trader.core.rust.model cimport Symbol_t
from nautilus_trader.core.rust.model cimport TradeId_t
from nautilus_trader.core.rust.model cimport Venue_t
from nautilus_trader.core.rust.model cimport VenueOrderId_t


cdef class Symbol:
    cdef Symbol_t _mem

    cdef readonly str value
    """The identifier (ID) value.\n\n:returns: `str`"""


cdef class Venue:
    cdef Venue_t _mem

    cdef readonly str value
    """The identifier (ID) value.\n\n:returns: `str`"""


cdef class InstrumentId:
    cdef InstrumentId_t _mem

    cdef readonly Symbol symbol
    """The instrument ticker symbol.\n\n:returns: `Symbol`"""
    cdef readonly Venue venue
    """The instrument trading venue.\n\n:returns: `Venue`"""
    cdef readonly str value
    """The identifier (ID) value.\n\n:returns: `str`"""

    @staticmethod
    cdef InstrumentId from_raw_c(InstrumentId_t raw)

    @staticmethod
    cdef InstrumentId from_str_c(str value)


cdef class ComponentId:
    cdef ComponentId_t _mem

    cdef readonly str value
    """The identifier (ID) value.\n\n:returns: `str`"""


cdef class ClientId(ComponentId):
    pass


cdef class TraderId(ComponentId):
    cpdef str get_tag(self)


cdef class StrategyId(ComponentId):
    cpdef str get_tag(self)
    cpdef bint is_external(self)
    @staticmethod
    cdef StrategyId external_c()


cdef class AccountId:
    cdef AccountId_t _mem

    cdef readonly str value
    """The identifier (ID) value.\n\n:returns: `str`"""
    cdef readonly str issuer
    """The account issuer.\n\n:returns: `str`"""
    cdef readonly str number
    """The account number.\n\n:returns: `str`"""

    @staticmethod
    cdef AccountId from_str_c(str value)


cdef class ClientOrderId:
    cdef ClientOrderId_t _mem

    cdef readonly str value
    """The identifier (ID) value.\n\n:returns: `str`"""


cdef class ClientOrderLinkId:
    cdef ClientOrderLinkId_t _mem

    cdef readonly str value
    """The identifier (ID) value.\n\n:returns: `str`"""


cdef class VenueOrderId:
    cdef VenueOrderId_t _mem

    cdef readonly str value
    """The identifier (ID) value.\n\n:returns: `str`"""


cdef class OrderListId:
    cdef OrderListId_t _mem

    cdef readonly str value
    """The identifier (ID) value.\n\n:returns: `str`"""


cdef class PositionId:
    cdef PositionId_t _mem

    cdef readonly str value
    """The identifier (ID) value.\n\n:returns: `str`"""

    cdef bint is_virtual_c(self) except *


cdef class TradeId:
    cdef TradeId_t _mem

    cdef readonly str value
    """The identifier (ID) value.\n\n:returns: `str`"""

    @staticmethod
    cdef TradeId from_raw_c(TradeId_t raw)
