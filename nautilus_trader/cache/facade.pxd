# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.strategy cimport Strategy


cdef class CacheDatabaseFacade:
    cdef Logger _log

    cpdef void close(self)
    cpdef void flush(self)
    cpdef list[str] keys(self, str pattern=*)
    cpdef dict load_all(self)
    cpdef dict load(self)
    cpdef dict load_currencies(self)
    cpdef dict load_instruments(self)
    cpdef dict load_synthetics(self)
    cpdef dict load_accounts(self)
    cpdef dict load_orders(self)
    cpdef dict load_positions(self)
    cpdef dict load_index_order_position(self)
    cpdef dict load_index_order_client(self)
    cpdef Currency load_currency(self, str code)
    cpdef Instrument load_instrument(self, InstrumentId instrument_id)
    cpdef SyntheticInstrument load_synthetic(self, InstrumentId instrument_id)
    cpdef Account load_account(self, AccountId account_id)
    cpdef Order load_order(self, ClientOrderId order_id)
    cpdef Position load_position(self, PositionId position_id)
    cpdef dict load_actor(self, ComponentId component_id)
    cpdef void delete_actor(self, ComponentId component_id)
    cpdef dict load_strategy(self, StrategyId strategy_id)
    cpdef void delete_strategy(self, StrategyId strategy_id)

    cpdef void add(self, str key, bytes value)
    cpdef void add_currency(self, Currency currency)
    cpdef void add_instrument(self, Instrument instrument)
    cpdef void add_synthetic(self, SyntheticInstrument instrument)
    cpdef void add_account(self, Account account)
    cpdef void add_order(self, Order order, PositionId position_id=*, ClientId client_id=*)
    cpdef void add_position(self, Position position)

    cpdef void index_venue_order_id(self, ClientOrderId client_order_id, VenueOrderId venue_order_id)
    cpdef void index_order_position(self, ClientOrderId client_order_id, PositionId position_id)

    cpdef void update_account(self, Account account)
    cpdef void update_order(self, Order order)
    cpdef void update_position(self, Position position)
    cpdef void update_actor(self, Actor actor)
    cpdef void update_strategy(self, Strategy strategy)

    cpdef void snapshot_order_state(self, Order order)
    cpdef void snapshot_position_state(self, Position position, uint64_t ts_snapshot, Money unrealized_pnl=*)

    cpdef void heartbeat(self, datetime timestamp)
