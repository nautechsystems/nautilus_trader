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
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionDatabase:
    cdef LoggerAdapter _log

    cdef readonly TraderId trader_id

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void flush(self) except *
    cpdef dict load_accounts(self)
    cpdef dict load_orders(self)
    cpdef dict load_positions(self)
    cpdef Account load_account(self, AccountId account_id)
    cpdef Order load_order(self, ClientOrderId order_id)
    cpdef Position load_position(self, PositionId position_id)
    cpdef dict load_strategy(self, TradingStrategy strategy)
    cpdef void delete_strategy(self, TradingStrategy strategy) except *

    cpdef void add_account(self, Account account) except *
    cpdef void add_order(self, Order order, PositionId position_id, StrategyId strategy_id) except *
    cpdef void add_position_id(self, PositionId position_id, ClientOrderId cl_ord_id, StrategyId strategy_id) except *
    cpdef void add_position(self, Position position, StrategyId strategy_id) except *

    cpdef void update_account(self, Account account) except *
    cpdef void update_order(self, Order order) except *
    cpdef void update_position(self, Position position) except *
    cpdef void update_strategy(self, TradingStrategy strategy) except *

    cpdef void add_stop_loss_id(self, ClientOrderId cl_ord_id) except *
    cpdef void add_take_profit_id(self, ClientOrderId cl_ord_id) except *
    cpdef void delete_stop_loss_id(self, ClientOrderId cl_ord_id) except *
    cpdef void delete_take_profit_id(self, ClientOrderId cl_ord_id) except *


cdef class BypassExecutionDatabase(ExecutionDatabase):
    pass
