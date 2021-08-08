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

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.position cimport PositionEvent
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.position cimport Position
from nautilus_trader.msgbus.message_bus cimport MessageBus
from nautilus_trader.trading.account cimport Account


cdef class PortfolioFacade:

# -- QUERIES ---------------------------------------------------------------------------------------  # noqa

    cdef readonly bint initialized
    """If the portfolio is initialized.\n\n:returns: `bool`"""

    cpdef Account account(self, Venue venue)

    cpdef dict initial_margins(self, Venue venue)
    cpdef dict maint_margins(self, Venue venue)
    cpdef dict unrealized_pnls(self, Venue venue)
    cpdef dict net_exposures(self, Venue venue)

    cpdef Money unrealized_pnl(self, InstrumentId instrument_id)
    cpdef Money net_exposure(self, InstrumentId instrument_id)
    cpdef object net_position(self, InstrumentId instrument_id)

    cpdef bint is_net_long(self, InstrumentId instrument_id) except *
    cpdef bint is_net_short(self, InstrumentId instrument_id) except *
    cpdef bint is_flat(self, InstrumentId instrument_id) except *
    cpdef bint is_completely_flat(self) except *


cdef class Portfolio(PortfolioFacade):
    cdef LoggerAdapter _log
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef MessageBus _msgbus
    cdef CacheFacade _cache

    cdef dict _unrealized_pnls
    cdef dict _net_positions
    cdef set _pending_calcs

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_account(self, Account account) except *

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void initialize_orders(self) except *
    cpdef void initialize_positions(self) except *
    cpdef void update_tick(self, QuoteTick tick) except *
    cpdef void update_order(self, OrderEvent event) except *
    cpdef void update_position(self, PositionEvent event) except *
    cpdef void reset(self) except *

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef object _net_position(self, InstrumentId instrument_id)
    cdef void _update_net_position(self, InstrumentId instrument_id, list positions_open) except *
    cdef bint _update_initial_margin(self, Venue venue, list orders_working) except *
    cdef bint _update_maint_margin(self, Venue venue, list positions_open) except *
    cdef Money _calculate_unrealized_pnl(self, InstrumentId instrument_id)
    cdef object _calculate_xrate_to_base(self, Instrument instrument, Account account, OrderSide side)
    cdef Price _get_last_price(self, Position position)
