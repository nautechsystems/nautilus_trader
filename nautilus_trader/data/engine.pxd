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

from cpython.datetime cimport datetime

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.commands cimport Connect
from nautilus_trader.common.commands cimport Disconnect
from nautilus_trader.common.commands cimport RequestData
from nautilus_trader.common.commands cimport Subscribe
from nautilus_trader.common.commands cimport Unsubscribe
from nautilus_trader.common.constants cimport *  # str constants
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.message cimport Command
from nautilus_trader.data.aggregation cimport TickBarAggregator
from nautilus_trader.data.aggregation cimport TimeBarAggregator
from nautilus_trader.data.cache cimport DataCache
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class DataEngine:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef Portfolio _portfolio
    cdef bint _use_previous_close
    cdef dict _clients
    cdef dict _instrument_handlers
    cdef dict _quote_tick_handlers
    cdef dict _trade_tick_handlers
    cdef dict _bar_aggregators
    cdef dict _bar_handlers

    cdef readonly DataCache cache
    cdef readonly int command_count
    cdef readonly int data_count

# -- REGISTRATIONS ---------------------------------------------------------------------------------

    cpdef void register_client(self, DataClient client) except *
    cpdef void register_strategy(self, TradingStrategy strategy) except *
    cpdef list registered_venues(self)

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef list subscribed_instruments(self)
    cpdef list subscribed_quote_ticks(self)
    cpdef list subscribed_trade_ticks(self)
    cpdef list subscribed_bars(self)

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void execute(self, Command command) except *
    cpdef void process(self, object data) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *
    cpdef void update_instruments(self, Venue venue) except *
    cpdef void update_instruments_all(self) except *

# -- COMMAND-HANDLERS ------------------------------------------------------------------------------

    cdef inline void _execute_command(self, Command command) except *
    cdef inline void _handle_connect(self, Connect command) except *
    cdef inline void _handle_disconnect(self, Disconnect command) except *
    cdef inline void _handle_subscribe(self, Subscribe command) except *
    cdef inline void _handle_unsubscribe(self, Unsubscribe command) except *
    cdef inline void _handle_request(self, RequestData command) except *
    cdef inline void _handle_subscribe_instrument(self, Symbol symbol, handler) except *
    cdef inline void _handle_subscribe_quote_ticks(self, Symbol symbol, handler) except *
    cdef inline void _handle_subscribe_trade_ticks(self, Symbol symbol, handler) except *
    cdef inline void _handle_subscribe_bars(self, BarType bar_type, handler) except *
    cdef inline void _handle_unsubscribe_instrument(self, Symbol symbol, handler) except *
    cdef inline void _handle_unsubscribe_quote_ticks(self, Symbol symbol, handler) except *
    cdef inline void _handle_unsubscribe_trade_ticks(self, Symbol symbol, handler) except *
    cdef inline void _handle_unsubscribe_bars(self, BarType bar_type, handler) except *
    cdef inline void _handle_request_instrument(self, Symbol symbol, callback) except *
    cdef inline void _handle_request_instruments(self, Venue venue, callback) except *
    cdef inline void _handle_request_quote_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback,
    ) except *
    cdef inline void _handle_request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback,
    ) except *
    cdef inline void _handle_request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback,
    ) except *


# -- DATA-HANDLERS ---------------------------------------------------------------------------------

    cdef inline void _handle_data(self, object data) except *
    cdef inline void _handle_instrument(self, Instrument instrument) except *
    cdef inline void _handle_instruments(self, list instruments) except *
    cdef inline void _handle_quote_tick(self, QuoteTick tick, bint send_to_handlers=*) except *
    cdef inline void _handle_quote_ticks(self, list ticks) except *
    cdef inline void _handle_trade_tick(self, TradeTick tick, bint send_to_handlers=*) except *
    cdef inline void _handle_trade_ticks(self, list ticks) except *
    cdef inline void _handle_bar(self, BarType bar_type, Bar bar, bint send_to_handlers=*) except *
    cdef inline void _handle_bars(self, BarType bar_type, list bars) except *

# -- INTERNAL --------------------------------------------------------------------------------------

    cpdef void _py_handle_bar(self, BarType bar_type, Bar bar) except *
    cdef inline void _internal_update_instruments(self, list instruments) except *
    cdef inline void _start_generating_bars(self, BarType bar_type, handler) except *
    cdef inline void _stop_generating_bars(self, BarType bar_type, handler) except *
    cdef inline void _add_quote_tick_handler(self, Symbol symbol, handler) except *
    cdef inline void _add_trade_tick_handler(self, Symbol symbol, handler) except *
    cdef inline void _add_bar_handler(self, BarType bar_type, handler) except *
    cdef inline void _add_instrument_handler(self, Symbol symbol, handler) except *
    cdef inline void _remove_quote_tick_handler(self, Symbol symbol, handler) except *
    cdef inline void _remove_trade_tick_handler(self, Symbol symbol, handler) except *
    cdef inline void _remove_bar_handler(self, BarType bar_type, handler) except *
    cdef inline void _remove_instrument_handler(self, Symbol symbol, handler) except *
    cdef inline void _bulk_build_tick_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback,
    ) except *


cdef class BulkTickBarBuilder:
    cdef TickBarAggregator aggregator
    cdef object callback
    cdef list bars

    cpdef void receive(self, list ticks) except *
    cpdef void _add_bar(self, BarType bar_type, Bar bar) except *


cdef class BulkTimeBarUpdater:
    cdef TimeBarAggregator aggregator
    cdef datetime start_time

    cpdef void receive(self, list ticks) except *
