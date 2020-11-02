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
from nautilus_trader.trading.portfolio cimport PortfolioFacade


cdef class Account:
    cdef list _events
    cdef Money _balance
    cdef Money _order_margin
    cdef Money _position_margin
    cdef PortfolioFacade _portfolio

    cdef readonly AccountId id
    """The accounts identifier.\n\n:returns: `AccountId`"""
    cdef readonly AccountType account_type
    """The accounts type.\n\n:returns: `AccountType`"""
    cdef readonly Currency currency
    """The accounts currency.\n\n:returns: `Currency`"""

    cpdef void register_portfolio(self, PortfolioFacade portfolio)
    cpdef void apply(self, AccountState event) except *
    cpdef void update_order_margin(self, Money margin) except *
    cpdef void update_position_margin(self, Money margin) except *

    cpdef Money balance(self)
    cpdef Money unrealized_pnl(self)
    cpdef Money margin_balance(self)
    cpdef Money margin_available(self)
    cpdef Money order_margin(self)
    cpdef Money position_margin(self)
