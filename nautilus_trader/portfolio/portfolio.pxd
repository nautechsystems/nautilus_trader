# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.accounting.manager cimport AccountsManager
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.position cimport PositionEvent
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.position cimport Position
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class Portfolio(PortfolioFacade):
    cdef Clock _clock
    cdef Logger _log
    cdef MessageBus _msgbus
    cdef Cache _cache
    cdef AccountsManager _accounts

    cdef Venue _venue
    cdef dict _unrealized_pnls
    cdef dict _net_positions
    cdef set _pending_calcs

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void set_specific_venue(self, Venue venue)
    cpdef void initialize_orders(self)
    cpdef void initialize_positions(self)
    cpdef void update_quote_tick(self, QuoteTick tick)
    cpdef void update_account(self, AccountState event)
    cpdef void update_order(self, OrderEvent event)
    cpdef void update_position(self, PositionEvent event)

# -- INTERNAL -------------------------------------------------------------------------------------

    cdef object _net_position(self, InstrumentId instrument_id)
    cdef void _update_net_position(self, InstrumentId instrument_id, list positions_open)
    cdef Money _calculate_unrealized_pnl(self, InstrumentId instrument_id)
    cdef Price _get_last_price(self, Position position)
    cdef double _calculate_xrate_to_base(self, Account account, Instrument instrument, OrderSide side)
