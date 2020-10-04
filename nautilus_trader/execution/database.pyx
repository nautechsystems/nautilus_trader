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

from nautilus_trader.common.logging cimport Logger


cdef class ExecutionDatabase:
    """
    The base class for all execution databases.

    """

    def __init__(self, TraderId trader_id not None, Logger logger not None):
        """
        Initialize a new instance of the ExecutionDatabase class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier to associate with the database.
        logger : Logger
            The logger for the database.

        """
        self.trader_id = trader_id
        self._log = LoggerAdapter(self.__class__.__name__, logger)

    cpdef void flush(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict load_accounts(self):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict load_orders(self):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict load_positions(self):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Account load_account(self, AccountId account_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Order load_order(self, ClientOrderId cl_ord_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Position load_position(self, PositionId position_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict load_strategy(self, StrategyId strategy_id):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void delete_strategy(self, StrategyId strategy_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void add_account(self, Account account) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void add_order(self, Order order, PositionId position_id, StrategyId strategy_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void add_stop_loss_id(self, ClientOrderId cl_ord_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void add_take_profit_id(self, ClientOrderId cl_ord_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void add_position(self, Position position, StrategyId strategy_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void add_position_id(self, PositionId position_id, ClientOrderId cl_ord_id, StrategyId strategy_id) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_account(self, Account event) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_order(self, Order order) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_position(self, Position position) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void update_strategy(self, TradingStrategy strategy) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class BypassExecutionDatabase(ExecutionDatabase):
    """
    Provides a bypass execution database which does nothing.

    """

    def __init__(self, TraderId trader_id not None, Logger logger not None):
        """
        Initialize a new instance of the BypassExecutionDatabase class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier to associate with the database.
        logger : Logger
            The logger for the database.

        """
        super().__init__(trader_id, logger)

    cpdef void flush(self) except *:
        # NO-OP
        pass

    cpdef dict load_accounts(self):
        return {}

    cpdef dict load_orders(self):
        return {}

    cpdef dict load_positions(self):
        return {}

    cpdef Account load_account(self, AccountId account_id):
        return None

    cpdef Order load_order(self, ClientOrderId cl_ord_id):
        return None

    cpdef Position load_position(self, PositionId position_id):
        return None

    cpdef dict load_strategy(self, StrategyId strategy_id):
        return {}

    cpdef void delete_strategy(self, StrategyId strategy_id) except *:
        # NO-OP
        pass

    cpdef void add_account(self, Account account) except *:
        # NO-OP
        pass

    cpdef void add_order(self, Order order, PositionId position_id, StrategyId strategy_id) except *:
        # NO-OP
        pass

    cpdef void add_stop_loss_id(self, ClientOrderId cl_ord_id) except *:
        # NO-OP
        pass

    cpdef void add_take_profit_id(self, ClientOrderId cl_ord_id) except *:
        # NO-OP
        pass

    cpdef void add_position(self, Position position, StrategyId strategy_id) except *:
        # NO-OP
        pass

    cpdef void add_position_id(self, PositionId position_id, ClientOrderId cl_ord_id, StrategyId strategy_id) except *:
        # NO-OP
        pass

    cpdef void update_account(self, Account event) except *:
        # NO-OP
        pass

    cpdef void update_order(self, Order order) except *:
        # NO-OP
        pass

    cpdef void update_position(self, Position position) except *:
        # NO-OP
        pass

    cpdef void update_strategy(self, TradingStrategy strategy) except *:
        # NO-OP
        pass
