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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.data.aggregation cimport TimeBarAggregator
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.data.messages cimport DataCommand
from nautilus_trader.data.messages cimport DataRequest
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport Subscribe
from nautilus_trader.data.messages cimport Unsubscribe
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.data cimport GenericData
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orderbook.book cimport OrderBookDeltas
from nautilus_trader.model.orderbook.book cimport OrderBookSnapshot
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.trading.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class DataEngine(Component):
    cdef bint _use_previous_close
    cdef dict _clients
    cdef dict _correlation_index
    cdef dict _instrument_handlers
    cdef dict _order_book_handlers
    cdef dict _order_book_delta_handlers
    cdef dict _quote_tick_handlers
    cdef dict _trade_tick_handlers
    cdef dict _bar_handlers
    cdef dict _data_handlers
    cdef dict _bar_aggregators
    cdef dict _order_book_intervals

    cdef readonly Portfolio portfolio
    """The portfolio wired to the engine.\n\n:returns: `Portfolio`"""
    cdef readonly Cache cache
    """The engines cache.\n\n:returns: `Cache`"""
    cdef readonly int command_count
    """The total count of data commands received by the engine.\n\n:returns: `int`"""
    cdef readonly int data_count
    """The total count of data stream objects received by the engine.\n\n:returns: `int`"""
    cdef readonly int request_count
    """The total count of data requests received by the engine.\n\n:returns: `int`"""
    cdef readonly int response_count
    """The total count of data responses received by the engine.\n\n:returns: `int`"""

    cpdef bint check_connected(self) except *
    cpdef bint check_disconnected(self) except *

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_client(self, DataClient client) except *
    cpdef void register_strategy(self, TradingStrategy strategy) except *
    cpdef void deregister_client(self, DataClient client) except *

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _on_start(self) except *
    cpdef void _on_stop(self) except *

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void execute(self, DataCommand command) except *
    cpdef void process(self, Data data) except *
    cpdef void send(self, DataRequest request) except *
    cpdef void receive(self, DataResponse response) except *

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef void _execute_command(self, DataCommand command) except *
    cdef void _handle_subscribe(self, DataClient client, Subscribe command) except *
    cdef void _handle_unsubscribe(self, DataClient client, Unsubscribe command) except *
    cdef void _handle_subscribe_instrument(self, MarketDataClient client, InstrumentId instrument_id, handler: callable) except *
    cdef void _handle_subscribe_order_book(self, MarketDataClient client, InstrumentId instrument_id, dict metadata, handler: callable) except *  # noqa
    cdef void _handle_subscribe_order_book_deltas(self, MarketDataClient client, InstrumentId instrument_id, dict metadata, handler: callable) except *  # noqa
    cdef void _handle_subscribe_quote_ticks(self, MarketDataClient client, InstrumentId instrument_id, handler: callable) except *
    cdef void _handle_subscribe_trade_ticks(self, MarketDataClient client, InstrumentId instrument_id, handler: callable) except *
    cdef void _handle_subscribe_bars(self, MarketDataClient client, BarType bar_type, handler: callable) except *
    cdef void _handle_subscribe_data(self, DataClient client, DataType data_type, handler: callable) except *
    cdef void _handle_unsubscribe_instrument(self, MarketDataClient client, InstrumentId instrument_id, handler: callable) except *
    cdef void _handle_unsubscribe_order_book(self, MarketDataClient client, InstrumentId instrument_id, dict metadata, handler: callable) except *  # noqa
    cdef void _handle_unsubscribe_quote_ticks(self, MarketDataClient client, InstrumentId instrument_id, handler: callable) except *
    cdef void _handle_unsubscribe_trade_ticks(self, MarketDataClient client, InstrumentId instrument_id, handler: callable) except *
    cdef void _handle_unsubscribe_bars(self, MarketDataClient client, BarType bar_type, handler: callable) except *
    cdef void _handle_unsubscribe_data(self, DataClient client, DataType data_type, handler: callable) except *
    cdef void _handle_request(self, DataRequest request) except *

# -- DATA HANDLERS ---------------------------------------------------------------------------------

    cdef void _handle_data(self, Data data) except *
    cdef void _handle_instrument(self, Instrument instrument) except *
    cdef void _handle_order_book_deltas(self, OrderBookDeltas deltas) except *
    cdef void _handle_order_book_snapshot(self, OrderBookSnapshot snapshot) except *
    cdef void _handle_quote_tick(self, QuoteTick tick) except *
    cdef void _handle_trade_tick(self, TradeTick tick) except *
    cdef void _handle_bar(self, Bar bar) except *
    cdef void _handle_generic_data(self, GenericData data) except *

# -- RESPONSE HANDLERS -----------------------------------------------------------------------------

    cdef void _handle_response(self, DataResponse response) except *
    cdef void _handle_instruments(self, list instruments, UUID correlation_id) except *
    cdef void _handle_quote_ticks(self, list ticks, UUID correlation_id) except *
    cdef void _handle_trade_ticks(self, list ticks, UUID correlation_id) except *
    cdef void _handle_bars(self, list bars, Bar partial, UUID correlation_id) except *

# -- INTERNAL --------------------------------------------------------------------------------------

    cpdef void _internal_update_instruments(self, list instruments) except *
    cpdef void _snapshot_order_book(self, TimeEvent snap_event) except *
    cdef void _start_bar_aggregator(self, MarketDataClient client, BarType bar_type) except *
    cdef void _hydrate_aggregator(self, MarketDataClient client, TimeBarAggregator aggregator, BarType bar_type) except *
    cdef void _stop_bar_aggregator(self, MarketDataClient client, BarType bar_type) except *
    cdef void _bulk_build_tick_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        callback: callable,
    ) except *
