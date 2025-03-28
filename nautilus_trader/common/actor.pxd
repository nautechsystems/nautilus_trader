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

from nautilus_trader.common.enums import UpdateCatalogMode

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.messages cimport DataCommand
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport RequestData
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.data cimport IndexPriceUpdate
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport MarkPriceUpdate
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.greeks cimport GreeksCalculator
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class Actor(Component):
    cdef object _executor
    cdef bint _log_events
    cdef bint _log_commands
    cdef set[type] _warning_events
    cdef dict[UUID4, object] _pending_requests
    cdef set[type] _pyo3_conversion_types
    cdef dict[str, type] _signal_classes
    cdef list[Indicator] _indicators
    cdef dict[InstrumentId, list[Indicator]] _indicators_for_quotes
    cdef dict[InstrumentId, list[Indicator]] _indicators_for_trades
    cdef dict[BarType, list[Indicator]] _indicators_for_bars

    cdef readonly PortfolioFacade portfolio
    """The read-only portfolio for the actor.\n\n:returns: `PortfolioFacade`"""
    cdef readonly config
    """The actors configuration.\n\n:returns: `NautilusConfig`"""
    cdef readonly Clock clock
    """The actors clock.\n\n:returns: `Clock`"""
    cdef readonly Logger log
    """The actors logger.\n\n:returns: `Logger`"""
    cdef readonly MessageBus msgbus
    """The message bus for the actor (if registered).\n\n:returns: `MessageBus` or ``None``"""
    cdef readonly CacheFacade cache
    """The read-only cache for the actor.\n\n:returns: `CacheFacade`"""
    cdef readonly GreeksCalculator greeks
    """The read-only greeks calculator for the actor.\n\n:returns: `GreeksCalculator`"""

    cpdef bint indicators_initialized(self)

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef dict[str, bytes] on_save(self)
    cpdef void on_load(self, dict[str, bytes] state)
    cpdef void on_start(self)
    cpdef void on_stop(self)
    cpdef void on_resume(self)
    cpdef void on_reset(self)
    cpdef void on_dispose(self)
    cpdef void on_degrade(self)
    cpdef void on_fault(self)
    cpdef void on_instrument_status(self, InstrumentStatus data)
    cpdef void on_instrument_close(self, InstrumentClose data)
    cpdef void on_instrument(self, Instrument instrument)
    cpdef void on_order_book_deltas(self, deltas)
    cpdef void on_order_book(self, OrderBook order_book)
    cpdef void on_quote_tick(self, QuoteTick tick)
    cpdef void on_trade_tick(self, TradeTick tick)
    cpdef void on_mark_price(self, MarkPriceUpdate mark_price)
    cpdef void on_index_price(self, IndexPriceUpdate index_price)
    cpdef void on_bar(self, Bar bar)
    cpdef void on_data(self, data)
    cpdef void on_signal(self, signal)
    cpdef void on_historical_data(self, data)
    cpdef void on_event(self, Event event)

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register_base(
        self,
        PortfolioFacade portfolio,
        MessageBus msgbus,
        CacheFacade cache,
        Clock clock,
    )

    cpdef void register_executor(self, loop, executor)
    cpdef void register_warning_event(self, type event)
    cpdef void deregister_warning_event(self, type event)
    cpdef void register_indicator_for_quote_ticks(self, InstrumentId instrument_id, Indicator indicator)
    cpdef void register_indicator_for_trade_ticks(self, InstrumentId instrument_id, Indicator indicator)
    cpdef void register_indicator_for_bars(self, BarType bar_type, Indicator indicator)

# -- ACTOR COMMANDS -------------------------------------------------------------------------------

    cpdef dict save(self)
    cpdef void load(self, dict state)
    cpdef void add_synthetic(self, SyntheticInstrument synthetic)
    cpdef void update_synthetic(self, SyntheticInstrument synthetic)
    cpdef queue_for_executor(self, func, tuple args=*, dict kwargs=*)
    cpdef run_in_executor(self, func, tuple args=*, dict kwargs=*)
    cpdef list queued_task_ids(self)
    cpdef list active_task_ids(self)
    cpdef bint has_queued_tasks(self)
    cpdef bint has_active_tasks(self)
    cpdef bint has_any_tasks(self)
    cpdef void cancel_task(self, task_id)
    cpdef void cancel_all_tasks(self)

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef void subscribe_data(self, DataType data_type, ClientId client_id=*, dict[str, object] params=*)
    cpdef void subscribe_instruments(self, Venue venue, ClientId client_id=*, dict[str, object] params=*)
    cpdef void subscribe_instrument(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void subscribe_order_book_deltas(
        self,
        InstrumentId instrument_id,
        BookType book_type=*,
        int depth=*,
        ClientId client_id=*,
        bint managed=*,
        bint pyo3_conversion=*,
        dict[str, object] params=*,
    )
    cpdef void subscribe_order_book_at_interval(
        self,
        InstrumentId instrument_id,
        BookType book_type=*,
        int depth=*,
        int interval_ms=*,
        ClientId client_id=*,
        bint managed=*,
        dict[str, object] params=*,
    )
    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void subscribe_mark_prices(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void subscribe_index_prices(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void subscribe_bars(self, BarType bar_type, ClientId client_id=*, bint await_partial=*, dict[str, object] params=*)
    cpdef void subscribe_instrument_status(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void subscribe_instrument_close(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void unsubscribe_data(self, DataType data_type, ClientId client_id=*, dict[str, object] params=*)
    cpdef void unsubscribe_instruments(self, Venue venue, ClientId client_id=*, dict[str, object] params=*)
    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void unsubscribe_order_book_at_interval(self, InstrumentId instrument_id, int interval_ms=*, ClientId client_id=*, dict[str, object] params=*)
    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void unsubscribe_mark_prices(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void unsubscribe_index_prices(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void unsubscribe_bars(self, BarType bar_type, ClientId client_id=*, dict[str, object] params=*)
    cpdef void unsubscribe_instrument_status(self, InstrumentId instrument_id, ClientId client_id=*, dict[str, object] params=*)
    cpdef void publish_data(self, DataType data_type, Data data)
    cpdef void publish_signal(self, str name, value, uint64_t ts_event=*)
    cpdef void subscribe_signal(self, str name=*)

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef UUID4 request_data(
        self,
        DataType data_type,
        ClientId client_id,
        datetime start=*,
        datetime end=*,
        int limit=*,
        callback=*,
        update_catalog_mode: UpdateCatalogMode | None = *,
        dict[str, object] params=*,
    )
    cpdef UUID4 request_instrument(
        self,
        InstrumentId instrument_id,
        datetime start=*,
        datetime end=*,
        ClientId client_id=*,
        callback=*,
        update_catalog_mode: UpdateCatalogMode | None = *,
        dict[str, object] params=*,
    )
    cpdef UUID4 request_instruments(
        self,
        Venue venue,
        datetime start=*,
        datetime end=*,
        ClientId client_id=*,
        callback=*,
        update_catalog_mode: UpdateCatalogMode | None = *,
        dict[str, object] params=*,
    )
    cpdef UUID4 request_order_book_snapshot(
        self,
        InstrumentId instrument_id,
        int limit=*,
        ClientId client_id=*,
        callback=*,
        dict[str, object] params=*,
    )
    cpdef UUID4 request_quote_ticks(
        self,
        InstrumentId instrument_id,
        datetime start=*,
        datetime end=*,
        int limit=*,
        ClientId client_id=*,
        callback=*,
        update_catalog_mode: UpdateCatalogMode | None = *,
        dict[str, object] params=*,
    )
    cpdef UUID4 request_trade_ticks(
        self,
        InstrumentId instrument_id,
        datetime start=*,
        datetime end=*,
        int limit=*,
        ClientId client_id=*,
        callback=*,
        update_catalog_mode: UpdateCatalogMode | None = *,
        dict[str, object] params=*,
    )
    cpdef UUID4 request_bars(
        self,
        BarType bar_type,
        datetime start=*,
        datetime end=*,
        int limit=*,
        ClientId client_id=*,
        callback=*,
        update_catalog_mode: UpdateCatalogMode | None = *,
        dict[str, object] params=*,
    )
    cpdef UUID4 request_aggregated_bars(
        self,
        list bar_types,
        datetime start=*,
        datetime end=*,
        int limit=*,
        ClientId client_id=*,
        callback=*,
        bint include_external_data=*,
        bint update_subscriptions=*,
        update_catalog_mode: UpdateCatalogMode | None = *,
        dict[str, object] params=*,
    )
    cpdef bint is_pending_request(self, UUID4 request_id)
    cpdef bint has_pending_requests(self)
    cpdef set pending_requests(self)

# -- HANDLERS -------------------------------------------------------------------------------------

    cpdef void handle_instrument(self, Instrument instrument)
    cpdef void handle_instruments(self, list instruments)
    cpdef void handle_order_book(self, OrderBook order_book)
    cpdef void handle_order_book_deltas(self, deltas)
    cpdef void handle_quote_tick(self, QuoteTick tick)
    cpdef void handle_quote_ticks(self, list ticks)
    cpdef void handle_trade_tick(self, TradeTick tick)
    cpdef void handle_trade_ticks(self, list ticks)
    cpdef void handle_mark_price(self, MarkPriceUpdate mark_price)
    cpdef void handle_index_price(self, IndexPriceUpdate index_price)
    cpdef void handle_bar(self, Bar bar)
    cpdef void handle_bars(self, list bars)
    cpdef void handle_data(self, Data data)
    cpdef void handle_signal(self, Data signal)
    cpdef void handle_instrument_status(self, InstrumentStatus data)
    cpdef void handle_instrument_close(self, InstrumentClose data)
    cpdef void handle_historical_data(self, data)
    cpdef void handle_event(self, Event event)

# -- HANDLERS -------------------------------------------------------------------------------------

    cpdef void _handle_data_response(self, DataResponse response)
    cpdef void _handle_instrument_response(self, DataResponse response)
    cpdef void _handle_instruments_response(self, DataResponse response)
    cpdef void _handle_quote_ticks_response(self, DataResponse response)
    cpdef void _handle_trade_ticks_response(self, DataResponse response)
    cpdef void _handle_bars_response(self, DataResponse response)
    cpdef void _handle_aggregated_bars_response(self, DataResponse response)
    cpdef void _finish_response(self, UUID4 request_id)
    cpdef void _handle_indicators_for_quote(self, list indicators, QuoteTick tick)
    cpdef void _handle_indicators_for_trade(self, list indicators, TradeTick tick)
    cpdef void _handle_indicators_for_bar(self, list indicators, Bar bar)

# -- EGRESS ---------------------------------------------------------------------------------------

    cdef void _send_data_cmd(self, DataCommand command)
    cdef void _send_data_req(self, RequestData request)
