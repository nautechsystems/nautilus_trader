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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.data.base cimport DataCacheFacade
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionModified
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.trading.account cimport Account


cdef class PortfolioFacade:

# -- QUERIES ---------------------------------------------------------------------------------------  # noqa

    cpdef Account account(self, Venue venue)

    cpdef dict init_margins(self, Venue venue)
    cpdef dict maint_margins(self, Venue venue)
    cpdef dict unrealized_pnls(self, Venue venue)
    cpdef dict market_values(self, Venue venue)

    cpdef Money unrealized_pnl(self, Symbol symbol)
    cpdef Money market_value(self, Symbol symbol)
    cpdef object net_position(self, Symbol symbol)

    cpdef bint is_net_long(self, Symbol symbol) except *
    cpdef bint is_net_short(self, Symbol symbol) except *
    cpdef bint is_flat(self, Symbol symbol) except *
    cpdef bint is_completely_flat(self) except *


cdef class Portfolio(PortfolioFacade):
    cdef LoggerAdapter _log
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef DataCacheFacade _data

    cdef dict _ticks
    cdef dict _accounts
    cdef dict _orders_working
    cdef dict _positions_open
    cdef dict _positions_closed
    cdef dict _unrealized_pnls
    cdef dict _net_positions

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_cache(self, DataCacheFacade cache) except *
    cpdef void register_account(self, Account account) except *

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void initialize_orders(self, set orders) except *
    cpdef void initialize_positions(self, set positions) except *
    cpdef void update_tick(self, QuoteTick tick) except *
    cpdef void update_order(self, Order order) except *
    cpdef void update_position(self, PositionEvent event) except *
    cpdef void reset(self) except *

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline object _net_position(self, Symbol symbol)
    cdef inline set _symbols_open_for_venue(self, Venue venue)
    cdef inline void _handle_position_opened(self, PositionOpened event) except *
    cdef inline void _handle_position_modified(self, PositionModified event) except *
    cdef inline void _handle_position_closed(self, PositionClosed event) except *
    cdef inline void _update_net_position(self, Symbol symbol, set positions_open) except *
    cdef inline void _update_init_margin(self, Venue venue) except *
    cdef inline void _update_maint_margin(self, Venue venue) except *
    cdef Money _calculate_unrealized_pnl(self, Symbol symbol)
    cdef object _calculate_xrate(self, Instrument instrument, Account account, OrderSide side)
    cdef inline Price _get_last_price(self, Position position)
