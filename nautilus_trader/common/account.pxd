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

from nautilus_trader.core.types cimport ValidString
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.events cimport AccountStateEvent
from nautilus_trader.model.identifiers cimport Brokerage, AccountNumber, AccountId
from nautilus_trader.model.objects cimport Decimal, Money


cdef class Account:
    cdef list _events
    cdef readonly AccountStateEvent last_event
    cdef readonly int event_count

    cdef readonly AccountId id
    cdef readonly Brokerage broker
    cdef readonly AccountNumber account_number
    cdef readonly AccountType account_type
    cdef readonly Currency currency
    cdef readonly Money cash_balance
    cdef readonly Money cash_start_day
    cdef readonly Money cash_activity_day
    cdef readonly Money margin_used_liquidation
    cdef readonly Money margin_used_maintenance
    cdef readonly Decimal margin_ratio
    cdef readonly ValidString margin_call_status
    cdef readonly free_equity

    cdef readonly datetime last_updated

    cpdef list get_events(self)
    cpdef void apply(self, AccountStateEvent event) except *

    cdef Money _calculate_free_equity(self)
