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
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.order.base cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.account cimport Account


cdef class ExecutionCacheFacade:

# -- ACCOUNT QUERIES -------------------------------------------------------------------------------  # noqa

    cpdef Account account(self, AccountId account_id)
    cpdef Account account_for_venue(self, Venue venue)
    cpdef AccountId account_id(self, Venue venue)
    cpdef list accounts(self)

# -- IDENTIFIER QUERIES ----------------------------------------------------------------------------

    cpdef set client_order_ids(self, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set client_order_ids_working(self, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set client_order_ids_completed(self, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set position_ids(self, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set position_open_ids(self, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set position_closed_ids(self, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set strategy_ids(self)

# -- ORDER QUERIES ---------------------------------------------------------------------------------

    cpdef Order order(self, ClientOrderId client_order_id)
    cpdef ClientOrderId client_order_id(self, VenueOrderId venue_order_id)
    cpdef VenueOrderId venue_order_id(self, ClientOrderId client_order_id)
    cpdef list orders(self, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef list orders_working(self, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef list orders_completed(self, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef bint order_exists(self, ClientOrderId client_order_id) except *
    cpdef bint is_order_working(self, ClientOrderId client_order_id) except *
    cpdef bint is_order_completed(self, ClientOrderId client_order_id) except *
    cpdef int orders_total_count(self, InstrumentId instrument_id=*, StrategyId strategy_id=*) except *
    cpdef int orders_working_count(self, InstrumentId instrument_id=*, StrategyId strategy_id=*) except *
    cpdef int orders_completed_count(self, InstrumentId instrument_id=*, StrategyId strategy_id=*) except *

# -- POSITION QUERIES ------------------------------------------------------------------------------

    cpdef Position position(self, PositionId position_id)
    cpdef PositionId position_id(self, ClientOrderId client_order_id)
    cpdef list positions(self, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef list positions_open(self, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef list positions_closed(self, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef bint position_exists(self, PositionId position_id) except *
    cpdef bint is_position_open(self, PositionId position_id) except *
    cpdef bint is_position_closed(self, PositionId position_id) except *
    cpdef int positions_total_count(self, InstrumentId instrument_id=*, StrategyId strategy_id=*) except *
    cpdef int positions_open_count(self, InstrumentId instrument_id=*, StrategyId strategy_id=*) except *
    cpdef int positions_closed_count(self, InstrumentId instrument_id=*, StrategyId strategy_id=*) except *

# -- STRATEGY QUERIES ------------------------------------------------------------------------------

    cpdef StrategyId strategy_id_for_order(self, ClientOrderId client_order_id)
    cpdef StrategyId strategy_id_for_position(self, PositionId position_id)
