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

from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport IndexPriceUpdate
from nautilus_trader.model.data cimport MarkPriceUpdate
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Quantity


cdef class CacheFacade:
    """
    Provides a read-only facade for the common `Cache`.
    """

# -- GENERAL --------------------------------------------------------------------------------------

    cpdef bytes get(self, str key):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `get` must be implemented in the subclass")  # pragma: no cover

    cpdef void add(self, str key, bytes value):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `add` must be implemented in the subclass")  # pragma: no cover

# -- DATA QUERIES ---------------------------------------------------------------------------------

    cpdef list quote_ticks(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `quote_ticks` must be implemented in the subclass")  # pragma: no cover

    cpdef list trade_ticks(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `trade_ticks` must be implemented in the subclass")  # pragma: no cover

    cpdef list mark_prices(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `mark_prices` must be implemented in the subclass")  # pragma: no cover

    cpdef list index_prices(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `index_prices` must be implemented in the subclass")  # pragma: no cover

    cpdef list bars(self, BarType bar_type):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `bars` must be implemented in the subclass")  # pragma: no cover

    cpdef Price price(self, InstrumentId instrument_id, PriceType price_type):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `price` must be implemented in the subclass")  # pragma: no cover

    cpdef dict[InstrumentId, Price] prices(self, PriceType price_type):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `prices` must be implemented in the subclass")  # pragma: no cover

    cpdef OrderBook order_book(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `order_book` must be implemented in the subclass")  # pragma: no cover

    cpdef object own_order_book(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `own_order_book` must be implemented in the subclass")  # pragma: no cover

    cpdef dict[Decimal, list[Order]] own_bid_orders(self, InstrumentId instrument_id, set[OrderStatus] status = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `own_bid_book` must be implemented in the subclass")  # pragma: no cover

    cpdef dict[Decimal, list[Order]] own_ask_orders(self, InstrumentId instrument_id, set[OrderStatus] status = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `own_ask_book` must be implemented in the subclass")  # pragma: no cover

    cpdef QuoteTick quote_tick(self, InstrumentId instrument_id, int index=0):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `quote_tick` must be implemented in the subclass")  # pragma: no cover

    cpdef TradeTick trade_tick(self, InstrumentId instrument_id, int index=0):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `trade_tick` must be implemented in the subclass")  # pragma: no cover

    cpdef MarkPriceUpdate mark_price(self, InstrumentId instrument_id, int index=0):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `mark_price` must be implemented in the subclass")  # pragma: no cover

    cpdef IndexPriceUpdate index_price(self, InstrumentId instrument_id, int index=0):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `index_price` must be implemented in the subclass")  # pragma: no cover

    cpdef Bar bar(self, BarType bar_type, int index=0):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `bar` must be implemented in the subclass")  # pragma: no cover

    cpdef int book_update_count(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `book_update_count` must be implemented in the subclass")  # pragma: no cover

    cpdef int quote_tick_count(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `quote_tick_count` must be implemented in the subclass")  # pragma: no cover

    cpdef int trade_tick_count(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `trade_tick_count` must be implemented in the subclass")  # pragma: no cover

    cpdef int mark_price_count(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `mark_price_count` must be implemented in the subclass")  # pragma: no cover

    cpdef int index_price_count(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `index_price_count` must be implemented in the subclass")  # pragma: no cover

    cpdef int bar_count(self, BarType bar_type):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `bar_count` must be implemented in the subclass")  # pragma: no cover

    cpdef bint has_order_book(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `has_order_book` must be implemented in the subclass")  # pragma: no cover

    cpdef bint has_quote_ticks(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `has_quote_ticks` must be implemented in the subclass")  # pragma: no cover

    cpdef bint has_trade_ticks(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `has_trade_ticks` must be implemented in the subclass")  # pragma: no cover

    cpdef bint has_mark_prices(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `has_mark_prices` must be implemented in the subclass")  # pragma: no cover

    cpdef bint has_index_prices(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `has_index_prices` must be implemented in the subclass")  # pragma: no cover

    cpdef bint has_bars(self, BarType bar_type):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `has_bars` must be implemented in the subclass")  # pragma: no cover

    cpdef get_xrate(
        self,
        Venue venue,
        Currency from_currency,
        Currency to_currency,
        PriceType price_type=PriceType.MID,
    ):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `get_xrate` must be implemented in the subclass")  # pragma: no cover

    cpdef get_mark_xrate(
        self,
        Currency from_currency,
        Currency to_currency,
    ):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `get_mark_xrate` must be implemented in the subclass")  # pragma: no cover

    cpdef void set_mark_xrate(
        self,
        Currency from_currency,
        Currency to_currency,
        double xrate,
    ):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `set_mark_xrate` must be implemented in the subclass")  # pragma: no cover

    cpdef void clear_mark_xrate(
        self,
        Currency from_currency,
        Currency to_currency,
    ):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `clear_mark_xrate` must be implemented in the subclass")  # pragma: no cover

    cpdef void clear_mark_xrates(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `clear_mark_xrates` must be implemented in the subclass")  # pragma: no cover

# -- INSTRUMENT QUERIES ---------------------------------------------------------------------------

    cpdef Instrument instrument(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `instrument` must be implemented in the subclass")  # pragma: no cover

    cpdef list instrument_ids(self, Venue venue = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `instrument_ids` must be implemented in the subclass")  # pragma: no cover

    cpdef list instruments(self, Venue venue = None, str underlying = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `instruments` must be implemented in the subclass")  # pragma: no cover

# -- SYNTHETIC QUERIES ----------------------------------------------------------------------------

    cpdef SyntheticInstrument synthetic(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `synthetic` must be implemented in the subclass")  # pragma: no cover

    cpdef list synthetic_ids(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `synthetic_ids` must be implemented in the subclass")  # pragma: no cover

    cpdef list synthetics(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `synthetics` must be implemented in the subclass")  # pragma: no cover

# -- ACCOUNT QUERIES ------------------------------------------------------------------------------

    cpdef Account account(self, AccountId account_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `account` must be implemented in the subclass")  # pragma: no cover

    cpdef Account account_for_venue(self, Venue venue):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `account_for_venue` must be implemented in the subclass")  # pragma: no cover

    cpdef AccountId account_id(self, Venue venue):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `account_id` must be implemented in the subclass")  # pragma: no cover

    cpdef list accounts(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `accounts` must be implemented in the subclass")  # pragma: no cover

# -- IDENTIFIER QUERIES ---------------------------------------------------------------------------

    cpdef set client_order_ids(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `client_order_ids` must be implemented in the subclass")  # pragma: no cover

    cpdef set client_order_ids_open(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `client_order_ids_open` must be implemented in the subclass")  # pragma: no cover

    cpdef set client_order_ids_closed(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `client_order_ids_closed` must be implemented in the subclass")  # pragma: no cover

    cpdef set client_order_ids_emulated(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `client_order_ids_emulated` must be implemented in the subclass")  # pragma: no cover

    cpdef set client_order_ids_inflight(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `client_order_ids_inflight` must be implemented in the subclass")  # pragma: no cover

    cpdef set order_list_ids(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `order_list_ids` must be implemented in the subclass")  # pragma: no cover

    cpdef set position_ids(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `position_ids` must be implemented in the subclass")  # pragma: no cover

    cpdef set position_open_ids(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `position_open_ids` must be implemented in the subclass")  # pragma: no cover

    cpdef set position_closed_ids(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `position_closed_ids` must be implemented in the subclass")  # pragma: no cover

    cpdef set actor_ids(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `actor_ids` must be implemented in the subclass")  # pragma: no cover

    cpdef set strategy_ids(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `strategy_ids` must be implemented in the subclass")  # pragma: no cover

    cpdef set exec_algorithm_ids(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `exec_algorithm_ids` must be implemented in the subclass")  # pragma: no cover

# -- ORDER QUERIES --------------------------------------------------------------------------------

    cpdef Order order(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `order` must be implemented in the subclass")  # pragma: no cover

    cpdef ClientOrderId client_order_id(self, VenueOrderId venue_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `client_order_id` must be implemented in the subclass")  # pragma: no cover

    cpdef VenueOrderId venue_order_id(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `venue_order_id` must be implemented in the subclass")  # pragma: no cover

    cpdef ClientId client_id(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `client_id` must be implemented in the subclass")  # pragma: no cover

    cpdef list orders(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders` must be implemented in the subclass")  # pragma: no cover

    cpdef list orders_open(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders_open` must be implemented in the subclass")  # pragma: no cover

    cpdef list orders_closed(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders_closed` must be implemented in the subclass")  # pragma: no cover

    cpdef list orders_emulated(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders_emulated` must be implemented in the subclass")  # pragma: no cover

    cpdef list orders_inflight(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders_inflight` must be implemented in the subclass")  # pragma: no cover

    cpdef list orders_for_position(self, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders_for_position` must be implemented in the subclass")  # pragma: no cover

    cpdef bint order_exists(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `order_exists` must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_order_open(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `is_order_open` must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_order_closed(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `is_order_closed` must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_order_emulated(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `is_order_emulated` must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_order_inflight(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `is_order_inflight` must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_order_pending_cancel_local(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `is_order_pending_cancel_local` must be implemented in the subclass")  # pragma: no cover

    cpdef int orders_open_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders_open_count` must be implemented in the subclass")  # pragma: no cover

    cpdef int orders_closed_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders_closed_count` must be implemented in the subclass")  # pragma: no cover

    cpdef int orders_emulated_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders_emulated_count` must be implemented in the subclass")  # pragma: no cover

    cpdef int orders_inflight_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders_inflight_count` must be implemented in the subclass")  # pragma: no cover

    cpdef int orders_total_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders_total_count` must be implemented in the subclass")  # pragma: no cover

# -- ORDER_LIST_QUERIES ---------------------------------------------------------------------------

    cpdef OrderList order_list(self, OrderListId order_list_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `order_list` must be implemented in the subclass")  # pragma: no cover

    cpdef list order_lists(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `order_lists` must be implemented in the subclass")  # pragma: no cover

    cpdef bint order_list_exists(self, OrderListId order_list_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `order_list_exists` must be implemented in the subclass")  # pragma: no cover

# -- EXEC ALGORITHM QUERIES -----------------------------------------------------------------------

    cpdef list orders_for_exec_algorithm(self, ExecAlgorithmId exec_algorithm_id, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, OrderSide side = OrderSide.NO_ORDER_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders_for_exec_algorithm` must be implemented in the subclass")  # pragma: no cover

    cpdef list orders_for_exec_spawn(self, ClientOrderId exec_spawn_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `orders_for_exec_spawn` must be implemented in the subclass")  # pragma: no cover

    cpdef Quantity exec_spawn_total_quantity(self, ClientOrderId exec_spawn_id, bint active_only=False):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `exec_spawn_total_quantity` must be implemented in the subclass")  # pragma: no cover

    cpdef Quantity exec_spawn_total_filled_qty(self, ClientOrderId exec_spawn_id, bint active_only=False):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `exec_spawn_total_filled_qty` must be implemented in the subclass")  # pragma: no cover

    cpdef Quantity exec_spawn_total_leaves_qty(self, ClientOrderId exec_spawn_id, bint active_only=False):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `exec_spawn_total_leaves_qty` must be implemented in the subclass")  # pragma: no cover

# -- POSITION QUERIES -----------------------------------------------------------------------------

    cpdef Position position(self, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `position` must be implemented in the subclass")  # pragma: no cover

    cpdef Position position_for_order(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `position_for_order` must be implemented in the subclass")  # pragma: no cover

    cpdef PositionId position_id(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `position_id` must be implemented in the subclass")  # pragma: no cover

    cpdef list position_snapshots(self, PositionId position_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `position_snapshots` must be implemented in the subclass")  # pragma: no cover

    cpdef list positions(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, PositionSide side = PositionSide.NO_POSITION_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `positions` must be implemented in the subclass")  # pragma: no cover

    cpdef bint position_exists(self, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `position_exists` must be implemented in the subclass")  # pragma: no cover

    cpdef list positions_open(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, PositionSide side = PositionSide.NO_POSITION_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `positions_open` must be implemented in the subclass")  # pragma: no cover

    cpdef list positions_closed(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `positions_closed` must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_position_open(self, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `is_position_open` must be implemented in the subclass")  # pragma: no cover

    cpdef bint is_position_closed(self, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `is_position_closed` must be implemented in the subclass")  # pragma: no cover

    cpdef int positions_open_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, PositionSide side = PositionSide.NO_POSITION_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `positions_open_count` must be implemented in the subclass")  # pragma: no cover

    cpdef int positions_closed_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `positions_closed_count` must be implemented in the subclass")  # pragma: no cover

    cpdef int positions_total_count(self, Venue venue = None, InstrumentId instrument_id = None, StrategyId strategy_id = None, PositionSide side = PositionSide.NO_POSITION_SIDE):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `positions_total_count` must be implemented in the subclass")  # pragma: no cover

# -- STRATEGY QUERIES -----------------------------------------------------------------------------

    cpdef StrategyId strategy_id_for_order(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `strategy_id_for_order` must be implemented in the subclass")  # pragma: no cover

    cpdef StrategyId strategy_id_for_position(self, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `strategy_id_for_position` must be implemented in the subclass")  # pragma: no cover

# -- GREEKS QUERIES -----------------------------------------------------------------------------

    cpdef void add_greeks(self, object greeks):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `add_greeks` must be implemented in the subclass")  # pragma: no cover

    cpdef void add_yield_curve(self, object yield_curve):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `add_yield_curve` must be implemented in the subclass")  # pragma: no cover

    cpdef object greeks(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `greeks` must be implemented in the subclass")  # pragma: no cover

    cpdef object yield_curve(self, str curve_name):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `yield_curve` must be implemented in the subclass")  # pragma: no cover
