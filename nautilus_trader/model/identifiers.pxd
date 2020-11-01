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

from nautilus_trader.model.c_enums.account_type cimport AccountType


cdef class Identifier:
    cdef readonly str value
    """The identifier value.\n\n:returns: `str`"""


cdef class Symbol(Identifier):
    cdef readonly str code
    """The symbol code.\n\n:returns: `str`"""
    cdef readonly Venue venue
    """The symbol venue.\n\n:returns: `Venue`"""

    @staticmethod
    cdef Symbol from_string_c(str value)


cdef class Venue(Identifier):
    pass


cdef class Exchange(Venue):
    pass


cdef class Brokerage(Identifier):
    pass


cdef class IdTag(Identifier):
    pass


cdef class TraderId(Identifier):
    cdef readonly str name
    """The name identifier of the trader.\n\n:returns: `str`"""
    cdef readonly IdTag tag
    """The order identifier tag of the trader.\n\n:returns: `IdTag`"""

    @staticmethod
    cdef TraderId from_string_c(str value)


cdef class StrategyId(Identifier):
    cdef readonly str name
    """The name identifier of the strategy.\n\n:returns: `str`"""
    cdef readonly IdTag tag
    """The order identifier tag of the strategy.\n\n:returns: `str`"""

    @staticmethod
    cdef StrategyId null()

    @staticmethod
    cdef StrategyId from_string_c(str value)


cdef class Issuer(Identifier):
    pass


cdef class AccountId(Identifier):
    cdef readonly Issuer issuer
    """The account issuer.\n\n:returns: `Issuer`"""
    cdef readonly Identifier identifier
    """The account identifier value.\n\n:returns: `Identifier`"""
    cdef readonly AccountType account_type
    """The account type.\n\n:returns: `AccountType`"""

    cdef Venue issuer_as_venue(self)

    @staticmethod
    cdef AccountId from_string_c(str value)


cdef class BracketOrderId(Identifier):
    pass


cdef class ClientOrderId(Identifier):
    pass


cdef class ClientOrderLinkId(Identifier):
    pass


cdef class OrderId(Identifier):
    pass


cdef class PositionId(Identifier):

    @staticmethod
    cdef PositionId null()


cdef class ExecutionId(Identifier):
    pass


cdef class TradeMatchId(Identifier):
    pass
