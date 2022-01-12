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

from nautilus_trader.core.message cimport Event
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId


cdef class AccountState(Event):
    cdef readonly AccountId account_id
    """The account ID associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly AccountType account_type
    """The account type for the event.\n\n:returns: `AccountType`"""
    cdef readonly Currency base_currency
    """The account type for the event.\n\n:returns: `Currency` or ``None``"""
    cdef readonly list balances
    """The account balances.\n\n:returns: `list[AccountBalance]`"""
    cdef readonly list margins
    """The margin balances.\n\n:returns: `list[MarginBalance]`"""
    cdef readonly bint is_reported
    """If the state is reported from the exchange (otherwise system calculated).\n\n:returns: `bool`"""
    cdef readonly dict info
    """The additional implementation specific account information.\n\n:returns: `dict[str, object]`"""

    @staticmethod
    cdef AccountState from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(AccountState obj)
