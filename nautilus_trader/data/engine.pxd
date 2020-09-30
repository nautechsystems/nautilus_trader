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
from nautilus_trader.common.market cimport ExchangeRateCalculator
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.core.message cimport Response
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.data.aggregation cimport TickBarAggregator
from nautilus_trader.data.aggregation cimport TimeBarAggregator
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.network.identifiers cimport ClientId
from nautilus_trader.network.messages cimport DataResponse
from nautilus_trader.network.node_clients cimport MessageClient
from nautilus_trader.network.node_clients cimport MessageSubscriber
from nautilus_trader.serialization.base cimport DataSerializer
from nautilus_trader.serialization.base cimport InstrumentSerializer
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class DataEngine:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef ExchangeRateCalculator _exchange_calculator
    cdef dict _clients
    cdef bint _use_previous_close

    cdef dict _instruments
    cdef dict _instrument_handlers
    cdef dict _quote_ticks
    cdef dict _trade_ticks
    cdef dict _quote_tick_handlers
    cdef dict _trade_tick_handlers
    cdef dict _bars
    cdef dict _bar_aggregators
    cdef dict _bar_handlers

    cdef readonly int tick_capacity
    cdef readonly int bar_capacity

    cpdef void set_use_previous_close(self, bint setting)
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

# -- HANDLER METHODS -------------------------------------------------------------------------------

    cpdef void handle_instrument(self, Instrument instrument) except *
    cpdef void handle_instruments(self, list instruments) except *
    cpdef void handle_quote_tick(self, QuoteTick tick, bint send_to_handlers=*) except *
    cpdef void handle_quote_ticks(self, list ticks) except *
    cpdef void handle_trade_tick(self, TradeTick tick, bint send_to_handlers=*) except *
    cpdef void handle_trade_ticks(self, list ticks) except *
    cpdef void handle_bar(self, BarType bar_type, Bar bar, bint send_to_handlers=*) except *
    cpdef void handle_bars(self, BarType bar_type, list bars) except *

# -- QUERY METHODS ---------------------------------------------------------------------------------

    cpdef list subscribed_instruments(self)
    cpdef list subscribed_quote_ticks(self)
    cpdef list subscribed_trade_ticks(self)
    cpdef list subscribed_bars(self)

    cpdef list symbols(self)
    cpdef list instruments(self)
    cpdef list quote_ticks(self, Symbol symbol)
    cpdef list trade_ticks(self, Symbol symbol)
    cpdef list bars(self, BarType bar_type)
    cpdef Instrument instrument(self, Symbol symbol)
    cpdef QuoteTick quote_tick(self, Symbol symbol, int index=*)
    cpdef TradeTick trade_tick(self, Symbol symbol, int index=*)
    cpdef Bar bar(self, BarType bar_type, int index=*)
    cpdef int quote_tick_count(self, Symbol symbol)
    cpdef int trade_tick_count(self, Symbol symbol)
    cpdef int bar_count(self, BarType bar_type)
    cpdef bint has_quote_ticks(self, Symbol symbol) except *
    cpdef bint has_trade_ticks(self, Symbol symbol) except *
    cpdef bint has_bars(self, BarType bar_type) except *

    cpdef double get_exchange_rate(
        self,
        Currency from_currency,
        Currency to_currency,
        PriceType price_type=*,
    )

# ------------------------------------------------------------------------------------------------ #

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


cdef class LiveDataEngine(DataEngine):
    cdef MessageClient _data_client
    cdef MessageSubscriber _data_subscriber
    cdef MessageSubscriber _tick_subscriber
    cdef DataSerializer _data_serializer
    cdef InstrumentSerializer _instrument_serializer
    cdef ObjectCache _cached_symbols
    cdef ObjectCache _cached_bar_types
    cdef dict _correlation_index

    cdef readonly TraderId trader_id
    cdef readonly ClientId client_id
    cdef readonly UUID last_request_id

    cpdef void _set_callback(self, UUID request_id, handler: callable) except *
    cpdef object _pop_callback(self, UUID correlation_id)
    cpdef void _handle_response(self, Response response) except *
    cpdef void _handle_data_response(self, DataResponse response) except *
    cpdef void _handle_instruments_py(self, list instruments) except *
    cpdef void _handle_tick_msg(self, str topic, bytes payload) except *
    cpdef void _handle_sub_msg(self, str topic, bytes payload) except *
