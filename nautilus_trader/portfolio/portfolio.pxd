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

from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.manager cimport AccountsManager
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.position cimport PositionEvent
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
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
    cdef object _config
    cdef bint _debug
    cdef bint _use_mark_prices
    cdef bint _use_mark_xrates
    cdef bint _convert_to_account_base_currency
    cdef uint64_t _min_account_state_logging_interval_ns
    cdef str _log_price
    cdef str _log_xrate

    cdef dict[InstrumentId, Money] _unrealized_pnls
    cdef dict[InstrumentId, Money] _realized_pnls
    cdef dict[PositionId, Money] _snapshot_sum_per_position
    cdef dict[PositionId, Money] _snapshot_last_per_position
    cdef dict[PositionId, int] _snapshot_processed_counts
    cdef dict[InstrumentId, Decimal] _net_positions
    cdef dict[PositionId, object] _bet_positions
    cdef object _index_bet_positions
    cdef set[InstrumentId] _pending_calcs
    cdef dict[InstrumentId, Price] _bar_close_prices
    cdef dict[AccountId, uint64_t] _last_account_state_log_ts

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void set_specific_venue(self, Venue venue)
    cpdef void set_use_mark_prices(self, bint value)
    cpdef void set_use_mark_xrates(self, bint value)
    cpdef void initialize_orders(self)
    cpdef void initialize_positions(self)
    cpdef void update_quote_tick(self, QuoteTick tick)
    cpdef void update_mark_price(self, object mark_price)
    cpdef void update_bar(self, Bar bar)
    cpdef void update_account(self, AccountState event)
    cpdef void update_order(self, OrderEvent event)
    cpdef void update_position(self, PositionEvent event)
    cpdef void on_order_event(self, OrderEvent event)
    cpdef void on_position_event(self, PositionEvent event)

# -- INTERNAL -------------------------------------------------------------------------------------

    cdef void _update_account(self, AccountState event)
    cdef void _update_instrument_id(self, InstrumentId instrument_id)
    cdef void _update_net_position(self, InstrumentId instrument_id, list positions_open)
    cdef object _net_position(self, InstrumentId instrument_id)
    cdef void _ensure_snapshot_pnls_cached_for(self, InstrumentId instrument_id)
    cdef Money _calculate_realized_pnl(self, InstrumentId instrument_id)
    cdef Money _calculate_unrealized_pnl(self, InstrumentId instrument_id, Price price=*)
    cdef Price _get_price(self, Position position)
    cdef _calculate_xrate_to_base(self, Account account, Instrument instrument, OrderSide side)
