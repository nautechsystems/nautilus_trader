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

from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.trading.portfolio cimport PortfolioFacade


cdef class Account:
    cdef list _events
    cdef dict _starting_balances
    cdef dict _balances
    cdef dict _balances_free
    cdef dict _balances_locked
    cdef dict _init_margins
    cdef dict _maint_margins
    cdef PortfolioFacade _portfolio

    cdef readonly AccountId id
    """The accounts identifier.\n\n:returns: `AccountId`"""
    cdef readonly Currency default_currency
    """The accounts default currency.\n\n:returns: `Currency`"""

    cdef AccountState last_event_c(self)
    cdef list events_c(self)
    cdef int event_count_c(self)

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void register_portfolio(self, PortfolioFacade portfolio)
    cpdef void apply(self, AccountState event) except *
    cpdef void update_init_margin(self, Money margin) except *
    cpdef void update_maint_margin(self, Money margin) except *

# -- QUERIES-CASH ----------------------------------------------------------------------------------

    cpdef list currencies(self)
    cpdef dict starting_balances(self)
    cpdef dict balances(self)
    cpdef dict balances_free(self)
    cpdef dict balances_locked(self)
    cpdef Money balance(self, Currency currency=*)
    cpdef Money balance_free(self, Currency currency=*)
    cpdef Money balance_locked(self, Currency currency=*)
    cpdef Money unrealized_pnl(self, Currency currency=*)
    cpdef Money equity(self, Currency currency=*)

# -- QUERIES-MARGIN --------------------------------------------------------------------------------

    cpdef dict init_margins(self)
    cpdef dict maint_margins(self)
    cpdef Money init_margin(self, Currency currency=*)
    cpdef Money maint_margin(self, Currency currency=*)
    cpdef Money free_margin(self, Currency currency=*)

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline void _update_balances(
        self,
        list balances,
        list balances_free,
        list balances_locked,
    ) except *
