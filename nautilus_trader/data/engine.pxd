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
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
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
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class DataEngine:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef Portfolio _portfolio

    cdef dict _clients
    cdef bint _use_previous_close

    cdef dict _instrument_handlers
    cdef dict _quote_tick_handlers
    cdef dict _trade_tick_handlers
    cdef dict _bar_aggregators
    cdef dict _bar_handlers

    cdef readonly DataCache cache

    cpdef void connect(self) except *
    cpdef void disconnect(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *
    cpdef void update_instruments(self, Venue venue) except *
    cpdef void update_instruments_all(self) except *
    cpdef void _internal_update_instruments(self, list instruments) except *
    cpdef void request_instrument(self, Symbol symbol, callback) except *
    cpdef void request_instruments(self, Venue venue, callback) except *
    cpdef void request_quote_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback,
    ) except *
    cpdef void request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback,
    ) except *
    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback,
    ) except *
    cpdef void subscribe_instrument(self, Symbol symbol, handler) except *
    cpdef void subscribe_quote_ticks(self, Symbol symbol, handler) except *
    cpdef void subscribe_trade_ticks(self, Symbol symbol, handler) except *
    cpdef void subscribe_bars(self, BarType bar_type, handler) except *
    cpdef void unsubscribe_instrument(self, Symbol symbol, handler) except *
    cpdef void unsubscribe_quote_ticks(self, Symbol symbol, handler) except *
    cpdef void unsubscribe_trade_ticks(self, Symbol symbol, handler) except *
    cpdef void unsubscribe_bars(self, BarType bar_type, handler) except *

# -- REGISTRATION METHODS --------------------------------------------------------------------------

    cpdef void register_data_client(self, DataClient client) except *
    cpdef void register_strategy(self, TradingStrategy strategy) except *
    cpdef list registered_venues(self)

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef list subscribed_instruments(self)
    cpdef list subscribed_quote_ticks(self)
    cpdef list subscribed_trade_ticks(self)
    cpdef list subscribed_bars(self)

# -- HANDLER METHODS -------------------------------------------------------------------------------

    cpdef void handle_instrument(self, Instrument instrument) except *
    cpdef void handle_instruments(self, list instruments) except *
    cpdef void handle_quote_tick(self, QuoteTick tick, bint send_to_handlers=*) except *
    cpdef void handle_quote_ticks(self, list ticks) except *
    cpdef void handle_trade_tick(self, TradeTick tick, bint send_to_handlers=*) except *
    cpdef void handle_trade_ticks(self, list ticks) except *
    cpdef void handle_bar(self, BarType bar_type, Bar bar, bint send_to_handlers=*) except *
    cpdef void handle_bars(self, BarType bar_type, list bars) except *

# --------------------------------------------------------------------------------------------------

    cdef void _start_generating_bars(self, BarType bar_type, handler) except *
    cdef void _stop_generating_bars(self, BarType bar_type, handler) except *
    cdef void _add_quote_tick_handler(self, Symbol symbol, handler) except *
    cdef void _add_trade_tick_handler(self, Symbol symbol, handler) except *
    cdef void _add_bar_handler(self, BarType bar_type, handler) except *
    cdef void _add_instrument_handler(self, Symbol symbol, handler) except *
    cdef void _remove_quote_tick_handler(self, Symbol symbol, handler) except *
    cdef void _remove_trade_tick_handler(self, Symbol symbol, handler) except *
    cdef void _remove_bar_handler(self, BarType bar_type, handler) except *
    cdef void _remove_instrument_handler(self, Symbol symbol, handler) except *
    cdef void _bulk_build_tick_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback,
    ) except *
    cdef void _reset(self) except *


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
