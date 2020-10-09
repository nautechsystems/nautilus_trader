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
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class Account:
    cdef list _events
    cdef Portfolio _portfolio

    cdef readonly AccountId id
    cdef readonly AccountType account_type
    cdef readonly Currency currency
    cdef readonly Money balance           # balance includes realized PNL
    cdef readonly Money margin_balance    # balance + unrealized PNL
    cdef readonly Money margin_available  # margin_balance - order_margin - position_margin
    cdef readonly Money order_margin
    cdef readonly Money position_margin

    cpdef void register_portfolio(self, Portfolio portfolio) except *
    cpdef int event_count(self)
    cpdef list get_events(self)
    cpdef AccountState last_event(self)
    cpdef void apply(self, AccountState event) except *
