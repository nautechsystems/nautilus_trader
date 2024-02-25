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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.core.data cimport Data
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.data.messages cimport DataCommand
from nautilus_trader.data.messages cimport DataRequest
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport Subscribe
from nautilus_trader.data.messages cimport Unsubscribe
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport CustomData
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.data cimport OrderBookDepth10
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.data cimport VenueStatus
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class DataEngine(Component):
    cdef readonly Cache _cache
    cdef readonly DataClient _default_client
    cdef readonly object _catalog

    cdef readonly dict[ClientId, DataClient] _clients
    cdef readonly dict[Venue, DataClient] _routing_map
    cdef readonly dict _order_book_intervals
    cdef readonly dict[BarType, BarAggregator] _bar_aggregators
    cdef readonly dict[InstrumentId, list[SyntheticInstrument]] _synthetic_quote_feeds
    cdef readonly dict[InstrumentId, list[SyntheticInstrument]] _synthetic_trade_feeds
    cdef readonly list[InstrumentId] _subscribed_synthetic_quotes
    cdef readonly list[InstrumentId] _subscribed_synthetic_trades
    cdef readonly bint _time_bars_build_with_no_updates
    cdef readonly bint _time_bars_timestamp_on_close
    cdef readonly str _time_bars_interval_type
    cdef readonly bint _validate_data_sequence

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

    cpdef bint check_connected(self)
    cpdef bint check_disconnected(self)

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register_client(self, DataClient client)
    cpdef void register_default_client(self, DataClient client)
    cpdef void register_venue_routing(self, DataClient client, Venue venue)
    cpdef void deregister_client(self, DataClient client)

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _on_start(self)
    cpdef void _on_stop(self)

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef list subscribed_custom_data(self)
    cpdef list subscribed_instruments(self)
    cpdef list subscribed_order_book_deltas(self)
    cpdef list subscribed_order_book_snapshots(self)
    cpdef list subscribed_quote_ticks(self)
    cpdef list subscribed_trade_ticks(self)
    cpdef list subscribed_bars(self)
    cpdef list subscribed_instrument_status(self)
    cpdef list subscribed_instrument_close(self)
    cpdef list subscribed_synthetic_quotes(self)
    cpdef list subscribed_synthetic_trades(self)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void execute(self, DataCommand command)
    cpdef void process(self, Data data)
    cpdef void request(self, DataRequest request)
    cpdef void response(self, DataResponse response)

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cpdef void _execute_command(self, DataCommand command)
    cpdef void _handle_subscribe(self, DataClient client, Subscribe command)
    cpdef void _handle_unsubscribe(self, DataClient client, Unsubscribe command)
    cpdef void _handle_subscribe_instrument(self, MarketDataClient client, InstrumentId instrument_id)
    cpdef void _handle_subscribe_order_book_deltas(self, MarketDataClient client, InstrumentId instrument_id, dict metadata)  # noqa
    cpdef void _handle_subscribe_order_book_snapshots(self, MarketDataClient client, InstrumentId instrument_id, dict metadata)  # noqa
    cpdef void _setup_order_book(self, MarketDataClient client, InstrumentId instrument_id, dict metadata, bint only_deltas, bint managed)  # noqa
    cpdef void _handle_subscribe_quote_ticks(self, MarketDataClient client, InstrumentId instrument_id)
    cpdef void _handle_subscribe_synthetic_quote_ticks(self, InstrumentId instrument_id)
    cpdef void _handle_subscribe_trade_ticks(self, MarketDataClient client, InstrumentId instrument_id)
    cpdef void _handle_subscribe_synthetic_trade_ticks(self, InstrumentId instrument_id)
    cpdef void _handle_subscribe_bars(self, MarketDataClient client, BarType bar_type, bint await_partial)
    cpdef void _handle_subscribe_data(self, DataClient client, DataType data_type)
    cpdef void _handle_subscribe_venue_status(self, MarketDataClient client, Venue venue)
    cpdef void _handle_subscribe_instrument_status(self, MarketDataClient client, InstrumentId instrument_id)
    cpdef void _handle_subscribe_instrument_close(self, MarketDataClient client, InstrumentId instrument_id)
    cpdef void _handle_unsubscribe_instrument(self, MarketDataClient client, InstrumentId instrument_id)
    cpdef void _handle_unsubscribe_order_book_deltas(self, MarketDataClient client, InstrumentId instrument_id, dict metadata)  # noqa
    cpdef void _handle_unsubscribe_order_book_snapshots(self, MarketDataClient client, InstrumentId instrument_id, dict metadata)  # noqa
    cpdef void _handle_unsubscribe_quote_ticks(self, MarketDataClient client, InstrumentId instrument_id)
    cpdef void _handle_unsubscribe_trade_ticks(self, MarketDataClient client, InstrumentId instrument_id)
    cpdef void _handle_unsubscribe_bars(self, MarketDataClient client, BarType bar_type)
    cpdef void _handle_unsubscribe_data(self, DataClient client, DataType data_type)
    cpdef void _handle_request(self, DataRequest request)
    cpdef void _query_catalog(self, DataRequest request)

# -- DATA HANDLERS --------------------------------------------------------------------------------

    cpdef void _handle_data(self, Data data)
    cpdef void _handle_instrument(self, Instrument instrument)
    cpdef void _handle_order_book_delta(self, OrderBookDelta delta)
    cpdef void _handle_order_book_deltas(self, OrderBookDeltas deltas)
    cpdef void _handle_order_book_depth(self, OrderBookDepth10 depth)
    cpdef void _handle_quote_tick(self, QuoteTick tick)
    cpdef void _handle_trade_tick(self, TradeTick tick)
    cpdef void _handle_bar(self, Bar bar)
    cpdef void _handle_custom_data(self, CustomData data)
    cpdef void _handle_venue_status(self, VenueStatus data)
    cpdef void _handle_instrument_status(self, InstrumentStatus data)
    cpdef void _handle_close_price(self, InstrumentClose data)

# -- RESPONSE HANDLERS ----------------------------------------------------------------------------

    cpdef void _handle_response(self, DataResponse response)
    cpdef void _handle_instruments(self, list instruments)
    cpdef void _handle_quote_ticks(self, list ticks)
    cpdef void _handle_trade_ticks(self, list ticks)
    cpdef void _handle_bars(self, list bars, Bar partial)

# -- INTERNAL -------------------------------------------------------------------------------------

    cpdef void _internal_update_instruments(self, list instruments)
    cpdef void _update_order_book(self, Data data)
    cpdef void _snapshot_order_book(self, TimeEvent snap_event)
    cpdef void _start_bar_aggregator(self, MarketDataClient client, BarType bar_type, bint await_partial)
    cpdef void _stop_bar_aggregator(self, MarketDataClient client, BarType bar_type)
    cpdef void _update_synthetics_with_quote(self, list synthetics, QuoteTick update)
    cpdef void _update_synthetic_with_quote(self, SyntheticInstrument synthetic, QuoteTick update)
    cpdef void _update_synthetics_with_trade(self, list synthetics, TradeTick update)
    cpdef void _update_synthetic_with_trade(self, SyntheticInstrument synthetic, TradeTick update)
