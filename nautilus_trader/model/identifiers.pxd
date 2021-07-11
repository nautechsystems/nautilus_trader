# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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


cdef class Identifier:
    cdef readonly str value
    """The ID value.\n\n:returns: `str`"""

    cdef bint is_null(self) except *
    cdef bint not_null(self) except *


cdef class Symbol(Identifier):
    pass


cdef class Venue(Identifier):
    pass


cdef class InstrumentId(Identifier):
    cdef readonly Symbol symbol
    """The instrument ticker symbol.\n\n:returns: `Symbol`"""
    cdef readonly Venue venue
    """The instrument trading venue.\n\n:returns: `Venue`"""

    @staticmethod
    cdef InstrumentId from_str_c(str value)


cdef class TraderId(Identifier):
    cpdef str get_tag(self)


cdef class StrategyId(Identifier):
    cpdef str get_tag(self)


cdef class AccountId(Identifier):
    cdef readonly str issuer
    """The account issuer.\n\n:returns: `str`"""
    cdef readonly str number
    """The account number.\n\n:returns: `str`"""

    @staticmethod
    cdef AccountId from_str_c(str value)


cdef class ClientId(Identifier):
    pass


cdef class ClientOrderId(Identifier):
    pass


cdef class ClientOrderLinkId(Identifier):
    pass


cdef class VenueOrderId(Identifier):

    @staticmethod
    cdef VenueOrderId null_c()


cdef class PositionId(Identifier):

    @staticmethod
    cdef PositionId null_c()


cdef class ExecutionId(Identifier):
    pass
