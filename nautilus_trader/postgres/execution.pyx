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
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class PostgresExecutionDatabase(ExecutionDatabase):
    """
    Provides an execution database backed by Postgres.

    """

    def __init__(self, TraderId trader_id not None, Logger logger not None):
        """
        Initialize a new instance of the PostgresExecutionDatabase class.

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

    cpdef void add_order(self, Order order, PositionId position_id) except *:
        # NO-OP
        pass

    cpdef void add_position(self, Position position) except *:
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
