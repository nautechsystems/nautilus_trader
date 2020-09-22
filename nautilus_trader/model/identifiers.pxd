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

from nautilus_trader.core.types cimport Identifier
from nautilus_trader.model.c_enums.account_type cimport AccountType


cdef class Symbol(Identifier):
    cdef readonly str code
    cdef readonly Venue venue

    @staticmethod
    cdef Symbol from_string(str value)


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
    cdef readonly IdTag order_id_tag

    @staticmethod
    cdef TraderId from_string(str value)


cdef class StrategyId(Identifier):
    cdef readonly str name
    cdef readonly IdTag order_id_tag

    @staticmethod
    cdef StrategyId from_string(str value)


cdef class AccountId(Identifier):
    cdef readonly Brokerage broker
    cdef readonly AccountNumber account_number
    cdef readonly AccountType account_type

    @staticmethod
    cdef AccountId from_string(str value)


cdef class AccountNumber(Identifier):
    pass


cdef class BracketOrderId(Identifier):
    pass


cdef class ClientOrderId(Identifier):
    pass


cdef class OrderId(Identifier):
    pass


cdef class ClientPositionId(Identifier):
    pass


cdef class PositionId(Identifier):
    pass


cdef class ExecutionId(Identifier):
    pass


cdef class MatchId(Identifier):
    pass


cdef class InstrumentId(Identifier):
    pass
