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
    """The identifier value.\n\n:returns: `str`"""


cdef class Symbol(Identifier):
    pass


cdef class Venue(Identifier):
    cdef readonly str broker
    """The optional broker name.\n\n:returns: `str` or None"""
    cdef readonly ClientId client_id
    """The client identifier for routing.\n\n:returns: `ClientId`"""


cdef class InstrumentId(Identifier):
    cdef readonly Symbol symbol
    """The instrument ticker symbol.\n\n:returns: `Symbol`"""
    cdef readonly Venue venue
    """The instrument trading venue.\n\n:returns: `Venue`"""

    @staticmethod
    cdef InstrumentId from_str_c(str value)


cdef class IdTag(Identifier):
    pass


cdef class TraderId(Identifier):
    cdef readonly str name
    """The name identifier of the trader.\n\n:returns: `str`"""
    cdef readonly IdTag tag
    """The order identifier tag of the trader.\n\n:returns: `IdTag`"""

    @staticmethod
    cdef TraderId from_str_c(str value)


cdef class StrategyId(Identifier):
    cdef readonly str name
    """The name identifier of the strategy.\n\n:returns: `str`"""
    cdef readonly IdTag tag
    """The order identifier tag of the strategy.\n\n:returns: `str`"""

    @staticmethod
    cdef StrategyId null_c()
    cdef inline bint is_null(self) except *
    cdef inline bint not_null(self) except *

    @staticmethod
    cdef StrategyId from_str_c(str value)


cdef class Issuer(Identifier):
    pass


cdef class AccountId(Identifier):
    cdef readonly Issuer issuer
    """The account issuer.\n\n:returns: `Issuer`"""
    cdef readonly Identifier identifier
    """The account identifier value.\n\n:returns: `Identifier`"""

    cdef Venue issuer_as_venue(self)

    @staticmethod
    cdef AccountId from_str_c(str value)


cdef class ClientId(Identifier):

    @staticmethod
    cdef ClientId from_str_c(str value)


cdef class ClientOrderId(Identifier):
    pass


cdef class ClientOrderLinkId(Identifier):
    pass


cdef class VenueOrderId(Identifier):

    @staticmethod
    cdef VenueOrderId null_c()
    cdef inline bint is_null(self) except *
    cdef inline bint not_null(self) except *


cdef class PositionId(Identifier):

    @staticmethod
    cdef PositionId null_c()
    cdef inline bint is_null(self) except *
    cdef inline bint not_null(self) except *


cdef class ExecutionId(Identifier):
    pass


cdef class TradeMatchId(Identifier):
    pass
