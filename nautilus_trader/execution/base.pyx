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

from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.order.base cimport Order
from nautilus_trader.trading.account cimport Account


cdef class ExecutionCacheFacade:
    """
    Provides a read-only facade for an `ExecutionCache`.
    """

# -- ACCOUNT QUERIES -------------------------------------------------------------------------------

    cpdef Account account(self, AccountId account_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Account account_for_venue(self, Venue venue):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef AccountId account_id(self, Venue venue):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list accounts(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- IDENTIFIER QUERIES ----------------------------------------------------------------------------

    cpdef set order_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set order_active_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set order_working_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set order_completed_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set position_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set position_open_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set position_closed_ids(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef set strategy_ids(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- ORDER QUERIES ---------------------------------------------------------------------------------

    cpdef Order order(self, ClientOrderId cl_ord_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef ClientOrderId cl_ord_id(self, OrderId order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef OrderId order_id(self, ClientOrderId cl_ord_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list orders(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list orders_active(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list orders_working(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list orders_completed(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint order_exists(self, ClientOrderId cl_ord_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_order_active(self, ClientOrderId cl_ord_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_order_working(self, ClientOrderId cl_ord_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_order_completed(self, ClientOrderId cl_ord_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int orders_total_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int orders_active_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int orders_working_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int orders_completed_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- POSITION QUERIES ------------------------------------------------------------------------------

    cpdef Position position(self, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef PositionId position_id(self, ClientOrderId cl_ord_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list positions(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list positions_open(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef list positions_closed(self, Symbol symbol=None, StrategyId strategy_id=None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint position_exists(self, PositionId position_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_position_open(self, PositionId position_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_position_closed(self, PositionId position_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int positions_total_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int positions_open_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef int positions_closed_count(self, Symbol symbol=None, StrategyId strategy_id=None) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- STRATEGY QUERIES ------------------------------------------------------------------------------

    cpdef StrategyId strategy_id_for_order(self, ClientOrderId cl_ord_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef StrategyId strategy_id_for_position(self, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")
