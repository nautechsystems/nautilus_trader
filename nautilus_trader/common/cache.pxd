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

from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport TraderId


cdef class IdentifierCache:
    cdef ObjectCache _cached_trader_ids
    cdef ObjectCache _cached_account_ids
    cdef ObjectCache _cached_strategy_ids
    cdef ObjectCache _cached_symbols

    cpdef TraderId get_trader_id(self, str value)
    cpdef AccountId get_account_id(self, str value)
    cpdef StrategyId get_strategy_id(self, str value)
    cpdef Symbol get_symbol(self, str value)
