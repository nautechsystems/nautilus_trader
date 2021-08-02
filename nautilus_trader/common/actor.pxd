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

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.message cimport Event
from nautilus_trader.data.messages cimport DataCommand
from nautilus_trader.data.messages cimport DataRequest
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.model.c_enums.book_level cimport BookLevel
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.base cimport Data
from nautilus_trader.model.data.base cimport DataType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.data.venue cimport InstrumentClosePrice
from nautilus_trader.model.data.venue cimport InstrumentStatusUpdate
from nautilus_trader.model.data.venue cimport VenueStatusUpdate
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orderbook.data cimport OrderBookData
from nautilus_trader.msgbus.message_bus cimport MessageBus


cdef class Actor(Component):
    cdef readonly TraderId trader_id
    """The trader ID associated with the actor.\n\n:returns: `TraderId`"""
    cdef readonly MessageBus msgbus
    """The message bus for the actor.\n\n:returns: `MessageBus`"""
    cdef readonly CacheFacade cache
    """The read-only cache for the actor.\n\n:returns: `CacheFacade`"""

    cdef void _check_registered(self) except *

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void on_start(self) except *
    cpdef void on_stop(self) except *
    cpdef void on_resume(self) except *
    cpdef void on_reset(self) except *
    cpdef void on_dispose(self) except *
    cpdef void on_instrument(self, Instrument instrument) except *
    cpdef void on_order_book_delta(self, OrderBookData data) except *
    cpdef void on_order_book(self, OrderBook order_book) except *
    cpdef void on_quote_tick(self, QuoteTick tick) except *
    cpdef void on_trade_tick(self, TradeTick tick) except *
    cpdef void on_bar(self, Bar bar) except *
    cpdef void on_data(self, Data data) except *
    cpdef void on_venue_status_update(self, VenueStatusUpdate update) except *
    cpdef void on_instrument_status_update(self, InstrumentStatusUpdate update) except *
    cpdef void on_instrument_close_price(self, InstrumentClosePrice update) except *
    cpdef void on_event(self, Event event) except *

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_base(
        self,
        TraderId trader_id,
        MessageBus msgbus,
        CacheFacade cache,
        Clock clock,
        Logger logger,
    ) except *

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_data(self, ClientId client_id, DataType data_type) except *
    cpdef void subscribe_strategy_data(self, type data_type=*, StrategyId strategy_id=*) except *
    cpdef void subscribe_instruments(self, Venue venue) except *
    cpdef void subscribe_instrument(self, InstrumentId instrument_id) except *
    cpdef void subscribe_order_book_deltas(
        self,
        InstrumentId instrument_id,
        BookLevel level=*,
        dict kwargs=*,
    ) except *
    cpdef void subscribe_order_book_snapshots(
        self,
        InstrumentId instrument_id,
        BookLevel level=*,
        int depth=*,
        int interval_ms=*,
        dict kwargs=*,
    ) except *
    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id) except *
    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id) except *
    cpdef void subscribe_bars(self, BarType bar_type) except *
    cpdef void subscribe_venue_status_updates(self, Venue venue) except *
    cpdef void subscribe_instrument_status_updates(self, InstrumentId instrument_id) except *
    cpdef void subscribe_instrument_close_prices(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_data(self, ClientId client_id, DataType data_type) except *
    cpdef void unsubscribe_strategy_data(self, type data_type, StrategyId strategy_id=*) except *
    cpdef void unsubscribe_instruments(self, Venue venue) except *
    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_order_book_snapshots(self, InstrumentId instrument_id, int interval_ms=*) except *
    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id) except *
    cpdef void unsubscribe_bars(self, BarType bar_type) except *
    cpdef void publish_data(self, Data data) except *

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request_data(self, ClientId client_id, DataType data_type) except *
    cpdef void request_quote_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime=*,
        datetime to_datetime=*,
    ) except *
    cpdef void request_trade_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime=*,
        datetime to_datetime=*,
    ) except *
    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime=*,
        datetime to_datetime=*,
    ) except *

# -- HANDLERS --------------------------------------------------------------------------------------

    cpdef void handle_instrument(self, Instrument instrument) except *
    cpdef void handle_order_book(self, OrderBook order_book) except *
    cpdef void handle_order_book_delta(self, OrderBookData data) except *
    cpdef void handle_quote_tick(self, QuoteTick tick, bint is_historical=*) except *
    cpdef void handle_quote_ticks(self, list ticks) except *
    cpdef void handle_trade_tick(self, TradeTick tick, bint is_historical=*) except *
    cpdef void handle_trade_ticks(self, list ticks) except *
    cpdef void handle_bar(self, Bar bar, bint is_historical=*) except *
    cpdef void handle_bars(self, list bars) except *
    cpdef void handle_data(self, Data data) except *
    cpdef void handle_venue_status_update(self, VenueStatusUpdate update) except *
    cpdef void handle_instrument_status_update(self, InstrumentStatusUpdate update) except *
    cpdef void handle_instrument_close_price(self, InstrumentClosePrice update) except *
    cpdef void handle_event(self, Event event) except *

    cpdef void _handle_data_response(self, DataResponse response) except *
    cpdef void _handle_quote_ticks_response(self, DataResponse response) except *
    cpdef void _handle_trade_ticks_response(self, DataResponse response) except *
    cpdef void _handle_bars_response(self, DataResponse response) except *

# -- EGRESS ----------------------------------------------------------------------------------------

    cdef void _send_data_cmd(self, DataCommand command) except *
    cdef void _send_data_req(self, DataRequest request) except *
