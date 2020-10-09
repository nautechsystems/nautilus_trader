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

from cpython.datetime cimport date

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionModified
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.trading.account cimport Account
from nautilus_trader.trading.calculators cimport ExchangeRateCalculator


cdef class Portfolio:
    cdef LoggerAdapter _log
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef ExchangeRateCalculator _xrate_calculator

    cdef dict _bid_quotes
    cdef dict _ask_quotes
    cdef dict _instruments
    cdef dict _accounts
    cdef dict _orders_working
    cdef dict _positions_open
    cdef dict _positions_closed
    cdef dict _position_margins
    cdef dict _order_margins
    cdef dict _unrealized_pnls
    cdef dict _open_values
    cdef Money _unrealized_pnl
    cdef Money _open_value
    cdef bint _calculated_latest_totals

    cdef readonly date date_now
    cdef readonly Currency base_currency

    cpdef void set_base_currency(self, Currency currency) except *
    cpdef void register_account(self, Account account) except *
    cpdef void update_instrument(self, Instrument instrument) except *
    cpdef void update_tick(self, QuoteTick tick) except *
    cpdef void update_orders_working(self, set orders) except *
    cpdef void update_order(self, Order order) except *
    cpdef void update_positions(self, set positions) except *
    cpdef void update_position(self, PositionEvent event) except *
    cpdef void reset(self) except *

    cpdef Money order_margin(self, Venue venue)
    cpdef Money position_margin(self, Venue venue)
    cpdef Money unrealized_pnl(self, Venue venue=*)
    cpdef Money open_value(self, Venue venue=*)

    cdef inline Money _money_zero(self)
    cdef inline double _get_xrate(self, Currency currency, PositionSide side) except *
    cdef inline void _update_order_margin(self, Venue venue) except *
    cdef inline void _update_position_margin(self, Venue venue) except *
    cdef inline void _calculate_open_value(self, Position position) except *
    cdef inline void _calculate_long_open_value_change(self, Venue venue, OrderSide fill_side, Money change) except *
    cdef inline void _calculate_short_open_value_change(self, Venue venue, OrderSide fill_side, Money change) except *
    cdef inline void _calculate_unrealized_pnl(self) except *
    cdef inline void _handle_position_opened(self, PositionOpened event) except *
    cdef inline void _handle_position_modified(self, PositionModified event) except *
    cdef inline void _handle_position_closed(self, PositionClosed event) except *
