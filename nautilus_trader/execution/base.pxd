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

from nautilus_trader.common.account cimport Account
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position


cdef class ExecutionCacheReadOnly:

    # -- General queries -------------------------------------------------------
    cpdef dict get_symbol_position_counts(self)
    cpdef Account get_account(self, AccountId account_id)
    cpdef bint is_net_long(self, Symbol symbol, StrategyId strategy_id=*) except *
    cpdef bint is_net_short(self, Symbol symbol, StrategyId strategy_id=*) except *
    cpdef bint is_flat(self, Symbol symbol=*, StrategyId strategy_id=*) except *
    cpdef bint is_completely_flat(self) except *

    # -- Identifier queries ----------------------------------------------------
    cpdef set order_ids(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef set order_working_ids(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef set order_completed_ids(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef set position_ids(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef set position_open_ids(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef set position_closed_ids(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef set strategy_ids(self)

    # -- Order queries ---------------------------------------------------------
    cpdef Order order(self, ClientOrderId cl_ord_id)
    cpdef list orders(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef list orders_working(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef list orders_completed(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef bint order_exists(self, ClientOrderId cl_ord_id) except *
    cpdef bint is_order_working(self, ClientOrderId cl_ord_id) except *
    cpdef bint is_order_completed(self, ClientOrderId cl_ord_id) except *
    cpdef int orders_total_count(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef int orders_working_count(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef int orders_completed_count(self, Symbol symbol=*, StrategyId strategy_id=*)

    # -- Position queries ------------------------------------------------------
    cpdef Position position(self, PositionId position_id)
    cpdef PositionId position_id(self, ClientOrderId cl_ord_id)
    cpdef list positions(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef list positions_open(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef list positions_closed(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef bint position_exists(self, PositionId position_id) except *
    cpdef bint position_exists_for_order(self, ClientOrderId cl_ord_id) except *
    cpdef bint position_indexed_for_order(self, ClientOrderId cl_ord_id) except *
    cpdef bint is_position_open(self, PositionId position_id) except *
    cpdef bint is_position_closed(self, PositionId position_id) except *
    cpdef int positions_total_count(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef int positions_open_count(self, Symbol symbol=*, StrategyId strategy_id=*)
    cpdef int positions_closed_count(self, Symbol symbol=*, StrategyId strategy_id=*)

    # -- Strategy queries ------------------------------------------------------
    cpdef StrategyId strategy_id_for_order(self, ClientOrderId cl_ord_id)
    cpdef StrategyId strategy_id_for_position(self, PositionId position_id)
