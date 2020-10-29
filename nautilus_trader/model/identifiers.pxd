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
    cdef str _value


cdef class Symbol(Identifier):
    cdef str _code
    cdef Venue _venue

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
    cdef str _name
    cdef IdTag _tag

    @staticmethod
    cdef TraderId from_string_c(str value)


cdef class StrategyId(Identifier):
    cdef str _name
    cdef IdTag _tag

    @staticmethod
    cdef StrategyId null()

    @staticmethod
    cdef StrategyId from_string_c(str value)


cdef class Issuer(Identifier):
    pass


cdef class AccountId(Identifier):
    cdef Issuer _issuer
    cdef Identifier _identifier
    cdef AccountType _account_type

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
