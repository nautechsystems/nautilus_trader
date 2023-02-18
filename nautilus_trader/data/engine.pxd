# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.timer cimport TimeEvent
from nautilus_trader.core.data cimport Data
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.data.messages cimport DataCommand
from nautilus_trader.data.messages cimport DataRequest
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport Subscribe
from nautilus_trader.data.messages cimport Unsubscribe
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.base cimport DataType
from nautilus_trader.model.data.base cimport GenericData
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.data.ticker cimport Ticker
from nautilus_trader.model.data.venue cimport InstrumentClose
from nautilus_trader.model.data.venue cimport InstrumentStatusUpdate
from nautilus_trader.model.data.venue cimport VenueStatusUpdate
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orderbook.data cimport OrderBookData


cdef class DataEngine(Component):
    cdef Cache _cache
    cdef DataClient _default_client

    cdef dict _clients
    cdef dict _routing_map
    cdef dict _order_book_intervals
    cdef dict _bar_aggregators
    cdef bint _time_bars_build_with_no_updates
    cdef bint _time_bars_timestamp_on_close
    cdef bint _validate_data_sequence

    cdef readonly bint debug
    """If debug mode is active (will provide extra debug logging).\n\n:returns: `bool`"""
    cdef readonly int command_count
    """The total count of data commands received by the engine.\n\n:returns: `int`"""
    cdef readonly int request_count
    """The total count of data requests received by the engine.\n\n:returns: `int`"""
    cdef readonly int response_count
    """The total count of data responses received by the engine.\n\n:returns: `int`"""
    cdef readonly int data_count
    """The total count of data stream objects received by the engine.\n\n:returns: `int`"""

    cpdef bint check_connected(self) except *
    cpdef bint check_disconnected(self) except *

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register_client(self, DataClient client) except *
    cpdef void register_default_client(self, DataClient client) except *
    cpdef void register_venue_routing(self, DataClient client, Venue venue) except *
    cpdef void deregister_client(self, DataClient client) except *

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _on_start(self) except *
    cpdef void _on_stop(self) except *

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef list subscribed_generic_data(self)
    cpdef list subscribed_instruments(self)
    cpdef list subscribed_order_book_deltas(self)
    cpdef list subscribed_order_book_snapshots(self)
    cpdef list subscribed_tickers(self)
    cpdef list subscribed_quote_ticks(self)
    cpdef list subscribed_trade_ticks(self)
    cpdef list subscribed_bars(self)
    cpdef list subscribed_instrument_status_updates(self)
    cpdef list subscribed_instrument_close(self)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void execute(self, DataCommand command) except *
    cpdef void process(self, Data data) except *
    cpdef void request(self, DataRequest request) except *
    cpdef void response(self, DataResponse response) except *

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cdef void _execute_command(self, DataCommand command) except *
    cdef void _handle_subscribe(self, DataClient client, Subscribe command) except *
    cdef void _handle_unsubscribe(self, DataClient client, Unsubscribe command) except *
    cdef void _handle_subscribe_instrument(self, MarketDataClient client, InstrumentId instrument_id) except *
    cdef void _handle_subscribe_order_book_deltas(self, MarketDataClient client, InstrumentId instrument_id, dict metadata) except *  # noqa
    cdef void _handle_subscribe_order_book_snapshots(self, MarketDataClient client, InstrumentId instrument_id, dict metadata) except *  # noqa
    cdef void _handle_subscribe_ticker(self, MarketDataClient client, InstrumentId instrument_id) except *
    cdef void _handle_subscribe_quote_ticks(self, MarketDataClient client, InstrumentId instrument_id) except *
    cdef void _handle_subscribe_trade_ticks(self, MarketDataClient client, InstrumentId instrument_id) except *
    cdef void _handle_subscribe_bars(self, MarketDataClient client, BarType bar_type) except *
    cdef void _handle_subscribe_data(self, DataClient client, DataType data_type) except *
    cdef void _handle_subscribe_venue_status_updates(self, MarketDataClient client, Venue venue) except *
    cdef void _handle_subscribe_instrument_status_updates(self, MarketDataClient client, InstrumentId instrument_id) except *
    cdef void _handle_subscribe_instrument_close(self, MarketDataClient client, InstrumentId instrument_id) except *
    cdef void _handle_unsubscribe_instrument(self, MarketDataClient client, InstrumentId instrument_id) except *
    cdef void _handle_unsubscribe_order_book_deltas(self, MarketDataClient client, InstrumentId instrument_id, dict metadata) except *  # noqa
    cdef void _handle_unsubscribe_order_book_snapshots(self, MarketDataClient client, InstrumentId instrument_id, dict metadata) except *  # noqa
    cdef void _handle_unsubscribe_ticker(self, MarketDataClient client, InstrumentId instrument_id) except *
    cdef void _handle_unsubscribe_quote_ticks(self, MarketDataClient client, InstrumentId instrument_id) except *
    cdef void _handle_unsubscribe_trade_ticks(self, MarketDataClient client, InstrumentId instrument_id) except *
    cdef void _handle_unsubscribe_bars(self, MarketDataClient client, BarType bar_type) except *
    cdef void _handle_unsubscribe_data(self, DataClient client, DataType data_type) except *
    cdef void _handle_request(self, DataRequest request) except *

# -- DATA HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_data(self, Data data) except *
    cdef void _handle_instrument(self, Instrument instrument) except *
    cdef void _handle_order_book_data(self, OrderBookData data) except *
    cdef void _handle_ticker(self, Ticker ticker) except *
    cdef void _handle_quote_tick(self, QuoteTick tick) except *
    cdef void _handle_trade_tick(self, TradeTick tick) except *
    cdef void _handle_bar(self, Bar bar) except *
    cdef void _handle_generic_data(self, GenericData data) except *
    cdef void _handle_venue_status_update(self, VenueStatusUpdate data) except *
    cdef void _handle_instrument_status_update(self, InstrumentStatusUpdate data) except *
    cdef void _handle_close_price(self, InstrumentClose data) except *

# -- RESPONSE HANDLERS ----------------------------------------------------------------------------

    cdef void _handle_response(self, DataResponse response) except *
    cdef void _handle_instruments(self, list instruments) except *
    cdef void _handle_quote_ticks(self, list ticks) except *
    cdef void _handle_trade_ticks(self, list ticks) except *
    cdef void _handle_bars(self, list bars, Bar partial) except *

# -- INTERNAL -------------------------------------------------------------------------------------

    cpdef void _internal_update_instruments(self, list instruments) except *
    cpdef void _maintain_order_book(self, OrderBookData data) except *
    cpdef void _snapshot_order_book(self, TimeEvent snap_event) except *
    cdef void _start_bar_aggregator(self, MarketDataClient client, BarType bar_type) except *
    cdef void _stop_bar_aggregator(self, MarketDataClient client, BarType bar_type) except *
