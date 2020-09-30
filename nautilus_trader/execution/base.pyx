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


cdef class ExecutionDatabaseReadOnly:
    """
    An abstract read-only facade for the execution database.
    """
    # -- QUERIES ---------------------------------------------------------------------------------------

    cpdef dict get_symbol_position_counts(self):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Account get_account(self, AccountId account_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_net_long(self, Symbol symbol, StrategyId strategy_id=None) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_net_short(self, Symbol symbol, StrategyId strategy_id=None) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_flat(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_completely_flat(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    # -- Identifier queries ----------------------------------------------------

    cpdef set stop_loss_ids(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set take_profit_ids(self, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_stop_loss(self, ClientOrderId cl_ord_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_take_profit(self, ClientOrderId cl_ord_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set flattening_ids(self):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set order_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set order_working_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set order_completed_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set position_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set position_open_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set position_closed_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set strategy_ids(self):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    # -- Order queries ---------------------------------------------------------
    cpdef Order order(self, ClientOrderId cl_ord_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list orders(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list orders_working(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list orders_completed(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    # -- Position queries ------------------------------------------------------
    cpdef Position position(self, PositionId position_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef PositionId position_id(self, ClientOrderId cl_ord_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list positions(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list positions_open(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list positions_closed(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint order_exists(self, ClientOrderId cl_ord_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_order_working(self, ClientOrderId cl_ord_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_order_completed(self, ClientOrderId cl_ord_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int orders_total_count(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int orders_working_count(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int orders_completed_count(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint position_exists(self, PositionId position_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint position_exists_for_order(self, ClientOrderId cl_ord_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint position_indexed_for_order(self, ClientOrderId cl_ord_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_position_open(self, PositionId position_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_position_closed(self, PositionId position_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int positions_total_count(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int positions_open_count(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int positions_closed_count(self, Symbol symbol=None, StrategyId strategy_id=None):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    # -- Strategy queries ------------------------------------------------------
    cpdef StrategyId strategy_id_for_order(self, ClientOrderId cl_ord_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef StrategyId strategy_id_for_position(self, PositionId position_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")
