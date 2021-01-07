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

from cpython.datetime cimport datetime

from nautilus_trader.common.component cimport Component
from nautilus_trader.common.messages cimport DataRequest
from nautilus_trader.common.messages cimport DataResponse
from nautilus_trader.common.messages cimport Subscribe
from nautilus_trader.common.messages cimport Unsubscribe
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.data.aggregation cimport TimeBarAggregator
from nautilus_trader.data.cache cimport DataCache
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.commands cimport VenueCommand
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class DataEngine(Component):
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

    cpdef bint check_initialized(self) except *
    cpdef bint check_disconnected(self) except *

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_client(self, DataClient client) except *
    cpdef void register_strategy(self, TradingStrategy strategy) except *
    cpdef void deregister_client(self, DataClient client) except *

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _on_start(self) except *
    cpdef void _on_stop(self) except *

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void execute(self, VenueCommand command) except *
    cpdef void process(self, data) except *
    cpdef void send(self, DataRequest request) except *
    cpdef void receive(self, DataResponse response) except *
    cpdef void update_instruments(self, Venue venue) except *
    cpdef void update_instruments_all(self) except *

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef inline void _execute_command(self, VenueCommand command) except *
    cdef inline void _handle_subscribe(self, DataClient client, Subscribe command) except *
    cdef inline void _handle_unsubscribe(self, DataClient client, Unsubscribe command) except *
    cdef inline void _handle_subscribe_instrument(self, DataClient client, Symbol symbol, handler: callable) except *
    cdef inline void _handle_subscribe_quote_ticks(self, DataClient client, Symbol symbol, handler: callable) except *
    cdef inline void _handle_subscribe_trade_ticks(self, DataClient client, Symbol symbol, handler: callable) except *
    cdef inline void _handle_subscribe_bars(self, DataClient client, BarType bar_type, handler: callable) except *
    cdef inline void _handle_unsubscribe_instrument(self, DataClient client, Symbol symbol, handler: callable) except *
    cdef inline void _handle_unsubscribe_quote_ticks(self, DataClient client, Symbol symbol, handler: callable) except *
    cdef inline void _handle_unsubscribe_trade_ticks(self, DataClient client, Symbol symbol, handler: callable) except *
    cdef inline void _handle_unsubscribe_bars(self, DataClient client, BarType bar_type, handler: callable) except *
    cdef inline void _handle_request(self, DataRequest request) except *

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
    cdef inline void _handle_bars(self, BarType bar_type, list bars, Bar partial, UUID correlation_id) except *

# -- INTERNAL --------------------------------------------------------------------------------------

    cpdef void _internal_update_instruments(self, list instruments) except *
    cdef inline void _start_bar_aggregator(self, DataClient client, BarType bar_type) except *
    cdef inline void _hydrate_aggregator(self, DataClient client, TimeBarAggregator aggregator, BarType bar_type) except *
    cdef inline void _stop_bar_aggregator(self, DataClient client, BarType bar_type) except *
    cdef inline void _bulk_build_tick_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback: callable,
    ) except *

# -- HANDLERS --------------------------------------------------------------------------------------

    cdef inline void _add_instrument_handler(self, Symbol symbol, handler: callable) except *
    cdef inline void _add_quote_tick_handler(self, Symbol symbol, handler: callable) except *
    cdef inline void _add_trade_tick_handler(self, Symbol symbol, handler: callable) except *
    cdef inline void _add_bar_handler(self, BarType bar_type, handler: callable) except *
    cdef inline void _remove_instrument_handler(self, Symbol symbol, handler: callable) except *
    cdef inline void _remove_quote_tick_handler(self, Symbol symbol, handler: callable) except *
    cdef inline void _remove_trade_tick_handler(self, Symbol symbol, handler: callable) except *
    cdef inline void _remove_bar_handler(self, BarType bar_type, handler: callable) except *
