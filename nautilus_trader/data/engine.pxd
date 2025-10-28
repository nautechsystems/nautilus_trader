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

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.persistence.catalog import ParquetDataCatalog

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.aggregation cimport BarAggregator
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.data.messages cimport DataCommand
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport RequestBars
from nautilus_trader.data.messages cimport RequestData
from nautilus_trader.data.messages cimport RequestInstrument
from nautilus_trader.data.messages cimport RequestInstruments
from nautilus_trader.data.messages cimport RequestOrderBookDepth
from nautilus_trader.data.messages cimport RequestOrderBookSnapshot
from nautilus_trader.data.messages cimport RequestQuoteTicks
from nautilus_trader.data.messages cimport RequestTradeTicks
from nautilus_trader.data.messages cimport SubscribeBars
from nautilus_trader.data.messages cimport SubscribeData
from nautilus_trader.data.messages cimport SubscribeFundingRates
from nautilus_trader.data.messages cimport SubscribeIndexPrices
from nautilus_trader.data.messages cimport SubscribeInstrument
from nautilus_trader.data.messages cimport SubscribeInstrumentClose
from nautilus_trader.data.messages cimport SubscribeInstruments
from nautilus_trader.data.messages cimport SubscribeInstrumentStatus
from nautilus_trader.data.messages cimport SubscribeMarkPrices
from nautilus_trader.data.messages cimport SubscribeOrderBook
from nautilus_trader.data.messages cimport SubscribeQuoteTicks
from nautilus_trader.data.messages cimport SubscribeTradeTicks
from nautilus_trader.data.messages cimport UnsubscribeBars
from nautilus_trader.data.messages cimport UnsubscribeData
from nautilus_trader.data.messages cimport UnsubscribeFundingRates
from nautilus_trader.data.messages cimport UnsubscribeIndexPrices
from nautilus_trader.data.messages cimport UnsubscribeInstrument
from nautilus_trader.data.messages cimport UnsubscribeInstrumentClose
from nautilus_trader.data.messages cimport UnsubscribeInstruments
from nautilus_trader.data.messages cimport UnsubscribeInstrumentStatus
from nautilus_trader.data.messages cimport UnsubscribeMarkPrices
from nautilus_trader.data.messages cimport UnsubscribeOrderBook
from nautilus_trader.data.messages cimport UnsubscribeQuoteTicks
from nautilus_trader.data.messages cimport UnsubscribeTradeTicks
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarAggregation
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport CustomData
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.data cimport FundingRateUpdate
from nautilus_trader.model.data cimport IndexPriceUpdate
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport MarkPriceUpdate
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.data cimport OrderBookDepth10
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument


cdef class DataEngine(Component):
    cdef readonly Cache _cache
    cdef readonly DataClient _default_client
    cdef readonly set[ClientId] _external_clients
    cdef readonly dict[str, ParquetDataCatalog] _catalogs

    cdef readonly dict[ClientId, DataClient] _clients
    cdef readonly dict[Venue, DataClient] _routing_map
    cdef readonly dict _order_book_intervals
    cdef readonly dict[BarType, BarAggregator] _bar_aggregators
    cdef readonly dict[InstrumentId, list[SyntheticInstrument]] _synthetic_quote_feeds
    cdef readonly dict[InstrumentId, list[SyntheticInstrument]] _synthetic_trade_feeds
    cdef readonly list[InstrumentId] _subscribed_synthetic_quotes
    cdef readonly list[InstrumentId] _subscribed_synthetic_trades
    cdef readonly dict[InstrumentId, list[OrderBookDelta]] _buffered_deltas_map
    cdef readonly dict[str, SnapshotInfo] _snapshot_info
    cdef readonly dict[UUID4, int] _query_group_n_responses
    cdef readonly dict[UUID4, list] _query_group_responses
    cdef readonly dict[UUID4, RequestData] _query_group_requests

    cdef readonly dict[InstrumentId, str] _topic_cache_deltas
    cdef readonly dict[InstrumentId, str] _topic_cache_quotes
    cdef readonly dict[InstrumentId, str] _topic_cache_trades
    cdef readonly dict[InstrumentId, str] _topic_cache_depth
    cdef readonly dict[InstrumentId, str] _topic_cache_status
    cdef readonly dict[BarType, str] _topic_cache_bars
    cdef readonly dict[InstrumentId, str] _topic_cache_mark_prices
    cdef readonly dict[InstrumentId, str] _topic_cache_index_prices
    cdef readonly dict[InstrumentId, str] _topic_cache_funding_rates
    cdef readonly dict[InstrumentId, str] _topic_cache_close_prices
    cdef readonly dict[tuple, str] _topic_cache_snapshots
    cdef readonly dict[tuple, str] _topic_cache_custom
    cdef readonly dict[DataType, str] _topic_cache_custom_simple
    cdef readonly dict[InstrumentId, str] _topic_cache_instruments

    cdef readonly str _time_bars_interval_type
    cdef readonly bint _time_bars_timestamp_on_close
    cdef readonly bint _time_bars_skip_first_non_full_bar
    cdef readonly bint _time_bars_build_with_no_updates
    cdef readonly dict[BarAggregation, object] _time_bars_origin_offset # pd.Timedelta or pd.DateOffset
    cdef readonly int _time_bars_build_delay
    cdef readonly bint _validate_data_sequence
    cdef readonly bint _buffer_deltas
    cdef readonly bint _emit_quotes_from_book
    cdef readonly bint _emit_quotes_from_book_depths

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
    cpdef set[ClientId] get_external_client_ids(self)
    cpdef bint _is_backtest_client(self, DataClient client)

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
    cpdef list subscribed_mark_prices(self)
    cpdef list subscribed_index_prices(self)
    cpdef list subscribed_funding_rates(self)
    cpdef list subscribed_bars(self)
    cpdef list subscribed_instrument_status(self)
    cpdef list subscribed_instrument_close(self)
    cpdef list subscribed_synthetic_quotes(self)
    cpdef list subscribed_synthetic_trades(self)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void stop_clients(self)
    cpdef void execute(self, DataCommand command)
    cpdef void process(self, Data data)
    cpdef void request(self, RequestData request)
    cpdef void response(self, DataResponse response)

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cpdef void _execute_command(self, DataCommand command)
    cpdef void _handle_subscribe(self, DataClient client, SubscribeData command)
    cpdef void _handle_unsubscribe(self, DataClient client, UnsubscribeData command)
    cpdef void _handle_subscribe_instruments(self, MarketDataClient client, SubscribeInstruments command)
    cpdef void _handle_subscribe_instrument(self, MarketDataClient client, SubscribeInstrument command)
    cpdef void _handle_subscribe_order_book(self, MarketDataClient client, SubscribeOrderBook command)
    cpdef void _setup_order_book(self, MarketDataClient client, SubscribeOrderBook command)
    cpdef void _create_new_book(self, InstrumentId instrument_id, BookType book_type)
    cpdef void _handle_subscribe_quote_ticks(self, MarketDataClient client, SubscribeQuoteTicks command)
    cpdef void _handle_subscribe_synthetic_quote_ticks(self, InstrumentId instrument_id)
    cpdef void _handle_subscribe_trade_ticks(self, MarketDataClient client, SubscribeTradeTicks command)
    cpdef void _handle_subscribe_mark_prices(self, MarketDataClient client, SubscribeMarkPrices command)
    cpdef void _handle_subscribe_index_prices(self, MarketDataClient client, SubscribeIndexPrices command)
    cpdef void _handle_subscribe_funding_rates(self, MarketDataClient client, SubscribeFundingRates command)
    cpdef void _handle_subscribe_synthetic_trade_ticks(self, InstrumentId instrument_id)
    cpdef void _handle_subscribe_bars(self, MarketDataClient client, SubscribeBars command)
    cpdef void _handle_subscribe_data(self, DataClient client, SubscribeData command)
    cpdef void _handle_subscribe_instrument_status(self, MarketDataClient client, SubscribeInstrumentStatus command)
    cpdef void _handle_subscribe_instrument_close(self, MarketDataClient client, SubscribeInstrumentClose command)
    cpdef void _handle_unsubscribe_instruments(self, MarketDataClient client, UnsubscribeInstruments command)
    cpdef void _handle_unsubscribe_instrument(self, MarketDataClient client, UnsubscribeInstrument command)
    cpdef void _handle_unsubscribe_order_book(self, MarketDataClient client, UnsubscribeOrderBook command)
    cpdef void _handle_unsubscribe_quote_ticks(self, MarketDataClient client, UnsubscribeQuoteTicks command)
    cpdef void _handle_unsubscribe_trade_ticks(self, MarketDataClient client, UnsubscribeTradeTicks command)
    cpdef void _handle_unsubscribe_mark_prices(self, MarketDataClient client, UnsubscribeMarkPrices command)
    cpdef void _handle_unsubscribe_index_prices(self, MarketDataClient client, UnsubscribeIndexPrices command)
    cpdef void _handle_unsubscribe_funding_rates(self, MarketDataClient client, UnsubscribeFundingRates command)
    cpdef void _handle_unsubscribe_bars(self, MarketDataClient client, UnsubscribeBars command)
    cpdef void _handle_unsubscribe_data(self, DataClient client, UnsubscribeData command)
    cpdef void _handle_unsubscribe_instrument_status(self, MarketDataClient client, UnsubscribeInstrumentStatus command)
    cpdef void _handle_unsubscribe_instrument_close(self, MarketDataClient client, UnsubscribeInstrumentClose command)

# -- REQUEST HANDLERS -----------------------------------------------------------------------------

    cpdef void _handle_request(self, RequestData request)
    cpdef void _handle_request_instruments(self, DataClient client, RequestInstruments request)
    cpdef void _handle_request_instrument(self, DataClient client, RequestInstrument request)
    cpdef void _handle_request_order_book_snapshot(self, DataClient client, RequestOrderBookSnapshot request)
    cpdef void _handle_request_order_book_depth(self, DataClient client, RequestOrderBookDepth request)
    cpdef void _date_range_client_request(self, DataClient client, RequestData request)
    cpdef void _handle_date_range_request(self, DataClient client, RequestData request)
    cpdef void _handle_request_quote_ticks(self, DataClient client, RequestQuoteTicks request)
    cpdef void _handle_request_trade_ticks(self, DataClient client, RequestTradeTicks request)
    cpdef void _handle_request_bars(self, DataClient client, RequestBars request)
    cpdef void _handle_request_data(self, DataClient client, RequestData request)
    cpdef void _query_catalog(self, RequestData request)

# -- DATA HANDLERS --------------------------------------------------------------------------------

    cpdef void _handle_data(self, Data data)
    cpdef void _handle_instrument(self, Instrument instrument, bint update_catalog = *, bint force_update_catalog = *)
    cpdef void _handle_order_book_delta(self, OrderBookDelta delta)
    cpdef void _handle_order_book_deltas(self, OrderBookDeltas deltas)
    cpdef void _handle_order_book_depth(self, OrderBookDepth10 depth)
    cpdef void _handle_quote_tick(self, QuoteTick tick)
    cpdef void _handle_trade_tick(self, TradeTick tick)
    cpdef void _handle_mark_price(self, MarkPriceUpdate mark_price)
    cpdef void _handle_index_price(self, IndexPriceUpdate index_price)
    cpdef void _handle_funding_rate(self, FundingRateUpdate funding_rate)
    cpdef void _handle_bar(self, Bar bar)
    cpdef void _handle_custom_data(self, CustomData data)
    cpdef void _handle_instrument_status(self, InstrumentStatus data)
    cpdef void _handle_close_price(self, InstrumentClose data)

# -- RESPONSE HANDLERS ----------------------------------------------------------------------------

    cpdef void _handle_response(self, DataResponse response)
    cpdef void _handle_instruments(self, list instruments, bint update_catalog = *, bint force_update_catalog = *)
    cpdef tuple[datetime, object] _catalog_last_timestamp(self, type data_cls, identifier: str | None = *)
    cpdef void _new_query_group(self, RequestData request, int n_components)
    cpdef DataResponse _handle_query_group(self, DataResponse response)
    cdef DataResponse _handle_query_group_aux(self, DataResponse response)
    cpdef Instrument _modify_instrument_properties(self, Instrument instrument, dict instrument_properties)
    cpdef void _check_bounds(self, DataResponse response)
    cpdef void _handle_quote_ticks(self, list ticks)
    cpdef void _handle_trade_ticks(self, list ticks)
    cpdef void _handle_order_book_depths(self, list depths)
    cpdef void _handle_bars(self, list bars)
    cpdef dict _handle_aggregated_bars(self, DataResponse response)
    cdef dict _handle_aggregated_bars_aux(self, DataResponse response)

# -- INTERNAL -------------------------------------------------------------------------------------

    cdef str _get_instruments_topic(self, InstrumentId instrument_id)
    cdef str _get_book_topic(self, type book_data_type, InstrumentId instrument_id)
    cdef str _get_deltas_topic(self, InstrumentId instrument_id)
    cdef str _get_depth_topic(self, InstrumentId instrument_id)
    cdef str _get_quotes_topic(self, InstrumentId instrument_id)
    cdef str _get_trades_topic(self, InstrumentId instrument_id)
    cdef str _get_status_topic(self, InstrumentId instrument_id)
    cdef str _get_mark_prices_topic(self, InstrumentId instrument_id)
    cdef str _get_index_prices_topic(self, InstrumentId instrument_id)
    cdef str _get_funding_rates_topic(self, InstrumentId instrument_id)
    cdef str _get_close_prices_topic(self, InstrumentId instrument_id)
    cdef str _get_snapshots_topic(self, InstrumentId instrument_id, int interval_ms)
    cdef str _get_custom_data_topic(self, DataType data_type, InstrumentId instrument_id = *)
    cdef str _get_bars_topic(self, BarType bar_type)

    cpdef void _internal_update_instruments(self, list instruments)
    cpdef void _update_order_book(self, Data data)
    cpdef void _snapshot_order_book(self, TimeEvent snap_event)
    cpdef void _publish_order_book(self, InstrumentId instrument_id, str topic)
    cpdef object _create_bar_aggregator(self, Instrument instrument, BarType bar_type, dict params)
    cpdef void _start_bar_aggregator(self, MarketDataClient client, SubscribeBars command)
    cpdef void _stop_bar_aggregator(self, MarketDataClient client, UnsubscribeBars command)
    cpdef void _update_synthetics_with_quote(self, list synthetics, QuoteTick update)
    cpdef void _update_synthetic_with_quote(self, SyntheticInstrument synthetic, QuoteTick update)
    cpdef void _update_synthetics_with_trade(self, list synthetics, TradeTick update)
    cpdef void _update_synthetic_with_trade(self, SyntheticInstrument synthetic, TradeTick update)


cdef class SnapshotInfo:
    cdef InstrumentId instrument_id
    cdef Venue venue
    cdef bint is_composite
    cdef str root
    cdef str topic
    cdef uint64_t interval_ms
