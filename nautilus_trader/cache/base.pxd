# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.data.ticker cimport Ticker
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.position cimport Position


cdef class CacheFacade:

# -- DATA QUERIES ---------------------------------------------------------------------------------  # noqa

    cpdef list tickers(self, InstrumentId instrument_id)
    cpdef list quote_ticks(self, InstrumentId instrument_id)
    cpdef list trade_ticks(self, InstrumentId instrument_id)
    cpdef list bars(self, BarType bar_type)
    cpdef Price price(self, InstrumentId instrument_id, PriceType price_type)
    cpdef OrderBook order_book(self, InstrumentId instrument_id)
    cpdef Ticker ticker(self, InstrumentId instrument_id, int index=*)
    cpdef QuoteTick quote_tick(self, InstrumentId instrument_id, int index=*)
    cpdef TradeTick trade_tick(self, InstrumentId instrument_id, int index=*)
    cpdef Bar bar(self, BarType bar_type, int index=*)
    cpdef int book_update_count(self, InstrumentId instrument_id) except *
    cpdef int ticker_count(self, InstrumentId instrument_id) except *
    cpdef int quote_tick_count(self, InstrumentId instrument_id) except *
    cpdef int trade_tick_count(self, InstrumentId instrument_id) except *
    cpdef int bar_count(self, BarType bar_type) except *
    cpdef bint has_order_book(self, InstrumentId instrument_id) except *
    cpdef bint has_tickers(self, InstrumentId instrument_id) except *
    cpdef bint has_quote_ticks(self, InstrumentId instrument_id) except *
    cpdef bint has_trade_ticks(self, InstrumentId instrument_id) except *
    cpdef bint has_bars(self, BarType bar_type) except *

    cpdef double get_xrate(
        self,
        Venue venue,
        Currency from_currency,
        Currency to_currency,
        PriceType price_type=*,
    ) except *

# -- INSTRUMENT QUERIES ---------------------------------------------------------------------------

    cpdef Instrument instrument(self, InstrumentId instrument_id)
    cpdef list instrument_ids(self, Venue venue=*)
    cpdef list instruments(self, Venue venue=*)

# -- ACCOUNT QUERIES ------------------------------------------------------------------------------

    cpdef Account account(self, AccountId account_id)
    cpdef Account account_for_venue(self, Venue venue)
    cpdef AccountId account_id(self, Venue venue)
    cpdef list accounts(self)

# -- IDENTIFIER QUERIES ---------------------------------------------------------------------------

    cpdef set client_order_ids(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set client_order_ids_open(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set client_order_ids_closed(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set client_order_ids_emulated(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set client_order_ids_inflight(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set position_ids(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set position_open_ids(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set position_closed_ids(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef set strategy_ids(self)

# -- ORDER QUERIES --------------------------------------------------------------------------------

    cpdef Order order(self, ClientOrderId client_order_id)
    cpdef ClientOrderId client_order_id(self, VenueOrderId venue_order_id)
    cpdef VenueOrderId venue_order_id(self, ClientOrderId client_order_id)
    cpdef list orders(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, OrderSide side=*)
    cpdef list orders_open(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, OrderSide side=*)
    cpdef list orders_closed(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, OrderSide side=*)
    cpdef list orders_emulated(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, OrderSide side=*)
    cpdef list orders_inflight(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, OrderSide side=*)
    cpdef list orders_for_position(self, PositionId position_id)
    cpdef bint order_exists(self, ClientOrderId client_order_id) except *
    cpdef bint is_order_open(self, ClientOrderId client_order_id) except *
    cpdef bint is_order_closed(self, ClientOrderId client_order_id) except *
    cpdef bint is_order_emulated(self, ClientOrderId client_order_id) except *
    cpdef bint is_order_inflight(self, ClientOrderId client_order_id) except *
    cpdef int orders_open_count(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, OrderSide side=*) except *
    cpdef int orders_closed_count(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, OrderSide side=*) except *
    cpdef int orders_emulated_count(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, OrderSide side=*) except *
    cpdef int orders_inflight_count(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, OrderSide side=*) except *
    cpdef int orders_total_count(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, OrderSide side=*) except *

# -- POSITION QUERIES -----------------------------------------------------------------------------

    cpdef Position position(self, PositionId position_id)
    cpdef Position position_for_order(self, ClientOrderId client_order_id)
    cpdef PositionId position_id(self, ClientOrderId client_order_id)
    cpdef list position_snapshots(self, PositionId position_id=*)
    cpdef list positions(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, PositionSide side=*)
    cpdef list positions_open(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, PositionSide side=*)
    cpdef list positions_closed(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*)
    cpdef bint position_exists(self, PositionId position_id) except *
    cpdef bint is_position_open(self, PositionId position_id) except *
    cpdef bint is_position_closed(self, PositionId position_id) except *
    cpdef int positions_open_count(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, PositionSide side=*) except *
    cpdef int positions_closed_count(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*) except *
    cpdef int positions_total_count(self, Venue venue=*, InstrumentId instrument_id=*, StrategyId strategy_id=*, PositionSide side=*) except *

# -- STRATEGY QUERIES -----------------------------------------------------------------------------

    cpdef StrategyId strategy_id_for_order(self, ClientOrderId client_order_id)
    cpdef StrategyId strategy_id_for_position(self, PositionId position_id)
