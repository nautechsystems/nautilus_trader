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
from nautilus_trader.common.messages cimport Connect
from nautilus_trader.common.messages cimport Disconnect
from nautilus_trader.common.messages cimport DataRequest
from nautilus_trader.common.messages cimport DataResponse
from nautilus_trader.common.messages cimport Subscribe
from nautilus_trader.common.messages cimport Unsubscribe
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.fsm cimport FiniteStateMachine
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.uuid cimport UUID
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
    cdef FiniteStateMachine _fsm
    cdef bint _use_previous_close
    cdef dict _clients
    cdef dict _correlation_index
    cdef dict _instrument_handlers
    cdef dict _quote_tick_handlers
    cdef dict _trade_tick_handlers
    cdef dict _bar_handlers
    cdef dict _bar_aggregators

    cdef readonly Portfolio portfolio
    """The portfolio wired to the engine.\n\n:returns: `Portfolio`"""
    cdef readonly DataCache cache
    """The engines data cache.\n\n:returns: `DataCache`"""
    cdef readonly int command_count
    """The total count of commands received by the engine.\n\n:returns: `int`"""
    cdef readonly int data_count
    """The total count of data objects received by the engine.\n\n:returns: `int`"""
    cdef readonly int request_count
    """The total count of requests received by the engine.\n\n:returns: `int`"""
    cdef readonly int response_count
    """The total count of responses received by the engine.\n\n:returns: `int`"""

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_client(self, DataClient client) except *
    cpdef void register_strategy(self, TradingStrategy strategy) except *

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void execute(self, Command command) except *
    cpdef void process(self, data) except *
    cpdef void send(self, DataRequest request) except *
    cpdef void receive(self, DataResponse response) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *
    cpdef void update_instruments(self, Venue venue) except *
    cpdef void update_instruments_all(self) except *

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef inline void _execute_command(self, Command command) except *
    cdef inline void _handle_connect(self, Connect command) except *
    cdef inline void _handle_disconnect(self, Disconnect command) except *
    cdef inline void _handle_subscribe(self, Subscribe command) except *
    cdef inline void _handle_unsubscribe(self, Unsubscribe command) except *
    cdef inline void _handle_request(self, DataRequest request) except *
    cdef inline void _handle_subscribe_instrument(self, Symbol symbol, handler) except *
    cdef inline void _handle_subscribe_quote_ticks(self, Symbol symbol, handler) except *
    cdef inline void _handle_subscribe_trade_ticks(self, Symbol symbol, handler) except *
    cdef inline void _handle_subscribe_bars(self, BarType bar_type, handler) except *
    cdef inline void _handle_unsubscribe_instrument(self, Symbol symbol, handler) except *
    cdef inline void _handle_unsubscribe_quote_ticks(self, Symbol symbol, handler) except *
    cdef inline void _handle_unsubscribe_trade_ticks(self, Symbol symbol, handler) except *
    cdef inline void _handle_unsubscribe_bars(self, BarType bar_type, handler) except *

# -- REQUEST HANDLERS ------------------------------------------------------------------------------

    cdef inline void _handle_request_instrument(self, Symbol symbol, UUID correlation_id) except *
    cdef inline void _handle_request_instruments(self, Venue venue, UUID correlation_id) except *
    cdef inline void _handle_request_quote_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *
    cdef inline void _handle_request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *
    cdef inline void _handle_request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *

# -- DATA HANDLERS ---------------------------------------------------------------------------------

    cdef inline void _handle_data(self, data) except *
    cdef inline void _handle_instrument(self, Instrument instrument) except *
    cdef inline void _handle_quote_tick(self, QuoteTick tick) except *
    cdef inline void _handle_trade_tick(self, TradeTick tick) except *
    cdef inline void _handle_bar(self, BarType bar_type, Bar bar) except *

# -- RESPONSE HANDLERS -----------------------------------------------------------------------------

    cdef inline void _handle_response(self, DataResponse response) except *
    cdef inline void _handle_instruments(self, list instruments, UUID correlation_id) except *
    cdef inline void _handle_quote_ticks(self, list ticks, UUID correlation_id) except *
    cdef inline void _handle_trade_ticks(self, list ticks, UUID correlation_id) except *
    cdef inline void _handle_bars(self, BarType bar_type, list bars, UUID correlation_id) except *

# -- INTERNAL --------------------------------------------------------------------------------------

    cpdef void _internal_update_instruments(self, list instruments) except *
    cdef inline void _start_bar_aggregator(self, BarType bar_type) except *
    cdef inline void _stop_bar_aggregator(self, BarType bar_type) except *
    cdef inline void _add_instrument_handler(self, Symbol symbol, handler) except *
    cdef inline void _add_quote_tick_handler(self, Symbol symbol, handler) except *
    cdef inline void _add_trade_tick_handler(self, Symbol symbol, handler) except *
    cdef inline void _add_bar_handler(self, BarType bar_type, handler) except *
    cdef inline void _remove_instrument_handler(self, Symbol symbol, handler) except *
    cdef inline void _remove_quote_tick_handler(self, Symbol symbol, handler) except *
    cdef inline void _remove_trade_tick_handler(self, Symbol symbol, handler) except *
    cdef inline void _remove_bar_handler(self, BarType bar_type, handler) except *
    cdef inline void _bulk_build_tick_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback,
    ) except *
