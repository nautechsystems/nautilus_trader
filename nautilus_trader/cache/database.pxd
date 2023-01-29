# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.strategy cimport Strategy


cdef class CacheDatabase:
    cdef LoggerAdapter _log

    cpdef void flush(self) except *
    cpdef dict load(self)
    cpdef dict load_currencies(self)
    cpdef dict load_instruments(self)
    cpdef dict load_accounts(self)
    cpdef dict load_orders(self)
    cpdef dict load_positions(self)
    cpdef dict load_submit_order_commands(self)
    cpdef dict load_submit_order_list_commands(self)
    cpdef Currency load_currency(self, str code)
    cpdef Instrument load_instrument(self, InstrumentId instrument_id)
    cpdef Account load_account(self, AccountId account_id)
    cpdef Order load_order(self, ClientOrderId order_id)
    cpdef Position load_position(self, PositionId position_id)
    cpdef dict load_actor(self, ComponentId component_id)
    cpdef void delete_actor(self, ComponentId component_id) except *
    cpdef dict load_strategy(self, StrategyId strategy_id)
    cpdef void delete_strategy(self, StrategyId strategy_id) except *
    cpdef SubmitOrder load_submit_order_command(self, ClientOrderId client_order_id)
    cpdef SubmitOrderList load_submit_order_list_command(self, OrderListId order_list_id)

    cpdef void add(self, str key, bytes value) except *
    cpdef void add_currency(self, Currency currency) except *
    cpdef void add_instrument(self, Instrument instrument) except *
    cpdef void add_account(self, Account account) except *
    cpdef void add_order(self, Order order) except *
    cpdef void add_position(self, Position position) except *
    cpdef void add_submit_order_command(self, SubmitOrder command) except *
    cpdef void add_submit_order_list_command(self, SubmitOrderList command) except *

    cpdef void update_account(self, Account account) except *
    cpdef void update_order(self, Order order) except *
    cpdef void update_position(self, Position position) except *
    cpdef void update_actor(self, Actor actor) except *
    cpdef void update_strategy(self, Strategy strategy) except *
