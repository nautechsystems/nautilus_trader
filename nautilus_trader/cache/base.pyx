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

from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.data.ticker cimport Ticker
from nautilus_trader.model.enums_c cimport PriceType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument


cdef class CacheFacade:
    """
    Provides a read-only facade for the common `Cache`.
    """

# -- DATA QUERIES ---------------------------------------------------------------------------------

    cpdef list tickers(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list quote_ticks(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list trade_ticks(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list bars(self, BarType bar_type):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef Price price(self, InstrumentId instrument_id, PriceType price_type):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef OrderBook order_book(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef Ticker ticker(self, InstrumentId instrument_id, int index=0):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef QuoteTick quote_tick(self, InstrumentId instrument_id, int index=0):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef TradeTick trade_tick(self, InstrumentId instrument_id, int index=0):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef Bar bar(self, BarType bar_type, int index=0):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int book_update_count(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int ticker_count(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int quote_tick_count(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int trade_tick_count(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int bar_count(self, BarType bar_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint has_order_book(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint has_tickers(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint has_quote_ticks(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint has_trade_ticks(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint has_bars(self, BarType bar_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef double get_xrate(
        self,
        Venue venue,
        Currency from_currency,
        Currency to_currency,
        PriceType price_type=PriceType.MID,
    ) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

# -- INSTRUMENT QUERIES ---------------------------------------------------------------------------

    cpdef Instrument instrument(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list instrument_ids(self, Venue venue = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list instruments(self, Venue venue = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

# -- ACCOUNT QUERIES ------------------------------------------------------------------------------

    cpdef Account account(self, AccountId account_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef Account account_for_venue(self, Venue venue):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef AccountId account_id(self, Venue venue):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list accounts(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

# -- IDENTIFIER QUERIES ---------------------------------------------------------------------------

    cpdef set client_order_ids(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef set client_order_ids_open(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef set client_order_ids_closed(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef set client_order_ids_emulated(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef set client_order_ids_inflight(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef set order_list_ids(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef set position_ids(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef set position_open_ids(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef set position_closed_ids(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef set strategy_ids(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

# -- ORDER QUERIES --------------------------------------------------------------------------------

    cpdef Order order(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef ClientOrderId client_order_id(self, VenueOrderId venue_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef VenueOrderId venue_order_id(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list orders(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list orders_open(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list orders_closed(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list orders_emulated(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list orders_inflight(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list orders_for_position(self, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint order_exists(self, ClientOrderId client_order_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_order_open(self, ClientOrderId client_order_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_order_closed(self, ClientOrderId client_order_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_order_emulated(self, ClientOrderId client_order_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_order_inflight(self, ClientOrderId client_order_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int orders_open_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int orders_closed_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int orders_emulated_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int orders_inflight_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int orders_total_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

# -- ORDER_LIST_QUERIES --------------------------------------------------------------------------------

    cpdef OrderList order_list(self, OrderListId order_list_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list order_lists(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint order_list_exists(self, OrderListId order_list_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

# -- POSITION QUERIES -----------------------------------------------------------------------------

    cpdef Position position(self, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef Position position_for_order(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef PositionId position_id(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list position_snapshots(self, PositionId position_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list positions(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, PositionSide side = PositionSide.NO_POSITION_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint position_exists(self, PositionId position_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list positions_open(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, PositionSide side = PositionSide.NO_POSITION_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef list positions_closed(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_position_open(self, PositionId position_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_position_closed(self, PositionId position_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int positions_open_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, PositionSide side = PositionSide.NO_POSITION_SIDE) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int positions_closed_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef int positions_total_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, PositionSide side = PositionSide.NO_POSITION_SIDE) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

# -- STRATEGY QUERIES -----------------------------------------------------------------------------

    cpdef StrategyId strategy_id_for_order(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef StrategyId strategy_id_for_position(self, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover
