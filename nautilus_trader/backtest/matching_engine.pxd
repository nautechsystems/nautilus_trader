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

from libc.stdint cimport uint64_t

from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.model.c_enums.book_type cimport BookType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.trailing_offset_type cimport TrailingOffsetType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orderbook.data cimport OrderBookData
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.market_to_limit cimport MarketToLimitOrder
from nautilus_trader.model.orders.trailing_stop_limit cimport TrailingStopLimitOrder
from nautilus_trader.model.orders.trailing_stop_market cimport TrailingStopMarketOrder
from nautilus_trader.model.position cimport Position


cdef class OrderMatchingEngine:
    cdef Clock _clock
    cdef LoggerAdapter _log
    cdef OrderBook _book
    cdef FillModel _fill_model
    cdef bint _reject_stop_orders
    cdef dict _account_ids
    cdef object _event_handler

    cdef readonly Venue venue
    """The venue for the matching engine.\n\n:returns: `Venue`"""
    cdef readonly Instrument instrument
    """The instrument for the matching engine.\n\n:returns: `Instrument`"""
    cdef readonly int product_id
    """The instruments product ID for the exchange.\n\n:returns: `int`"""
    cdef readonly BookType book_type
    """The order book type for the matching engine.\n\n:returns: `BookType`"""
    cdef readonly OMSType oms_type
    """The order management system type for the matching engine.\n\n:returns: `OMSType`"""
    cdef readonly CacheFacade cache
    """The cache for the matching engine.\n\n:returns: `CacheFacade`"""

    cdef Price _last
    cdef Price _last_bid
    cdef Price _last_ask
    cdef Bar _last_bid_bar
    cdef Bar _last_ask_bar
    cdef dict _order_index
    cdef list _orders_bid
    cdef list _orders_ask
    cdef dict _oto_orders
    cdef bint _bar_execution

    cdef int _position_count
    cdef int _order_count
    cdef int _execution_count

    cpdef void reset(self) except *
    cpdef Price best_bid_price(self)
    cpdef Price best_ask_price(self)
    cpdef OrderBook get_book(self)
    cpdef list get_open_orders(self)
    cpdef list get_open_bid_orders(self)
    cpdef list get_open_ask_orders(self)

    cpdef void process_order_book(self, OrderBookData data) except *
    cpdef void process_quote_tick(self, QuoteTick tick) except *
    cpdef void process_trade_tick(self, TradeTick tick) except *
    cpdef void process_bar(self, Bar bar) except *
    cdef void _process_trade_ticks_from_bar(self, Bar bar) except *
    cdef void _process_quote_ticks_from_bar(self) except *

# -- COMMAND HANDLING -----------------------------------------------------------------------------

    cpdef bint order_exists(self, ClientOrderId client_order_id) except *
    cpdef void process_order(self, Order order, AccountId account_id) except *
    cpdef void process_modify(self, ModifyOrder command, AccountId account_id) except *
    cpdef void process_cancel(self, CancelOrder command, AccountId account_id) except *
    cpdef void process_cancel_all(self, CancelAllOrders command, AccountId account_id) except *
    cdef void _process_market_order(self, MarketOrder order) except *
    cdef void _process_market_to_limit_order(self, MarketToLimitOrder order) except *
    cdef void _process_limit_order(self, LimitOrder order) except *
    cdef void _process_stop_market_order(self, Order order) except *
    cdef void _process_stop_limit_order(self, Order order) except *
    cdef void _process_trailing_stop_market_order(self, TrailingStopMarketOrder order) except *
    cdef void _process_trailing_stop_limit_order(self, TrailingStopLimitOrder order) except *
    cdef void _update_limit_order(self, Order order, Quantity qty, Price price) except *
    cdef void _update_stop_market_order(self, Order order, Quantity qty, Price trigger_price) except *
    cdef void _update_stop_limit_order(self, Order order, Quantity qty, Price price, Price trigger_price) except *

    cpdef void add_order(self, Order order) except *
    cdef void _add_order(self, Order order) except*
    cpdef void delete_order(self, Order order) except *
    cpdef void iterate(self, uint64_t timestamp_ns) except *
    cdef void _iterate_side(self, list orders, uint64_t timestamp_ns) except *
    cpdef void match_order(self, Order order) except *
    cpdef void match_limit_order(self, Order order) except *
    cpdef void match_stop_market_order(self, Order order) except *
    cpdef void match_stop_limit_order(self, Order order) except *
    cpdef bint is_limit_marketable(self, OrderSide side, Price price) except *
    cpdef bint is_limit_matched(self, OrderSide side, Price price) except *
    cpdef bint is_stop_marketable(self, OrderSide side, Price price) except *
    cpdef bint is_stop_triggered(self, OrderSide side, Price price) except *
    cpdef list determine_limit_price_and_volume(self, Order order)
    cpdef list determine_market_price_and_volume(self, Order order)

    cdef void fill_market_order(self, Order order, LiquiditySide liquidity_side) except *
    cdef void fill_limit_order(self, Order order, LiquiditySide liquidity_side) except *
    cdef void apply_fills(
        self,
        Order order,
        LiquiditySide liquidity_side,
        list fills,
        PositionId position_id,
        Position position,
    ) except *
    cdef void fill_order(
        self,
        Order order,
        PositionId venue_position_id,
        Position position,
        Quantity last_qty,
        Price last_px,
        LiquiditySide liquidity_side,
    ) except *
    cdef void _manage_trailing_stop(self, Order order) except *
    cdef Price _calculate_new_trailing_price_last(
        self,
        Order order,
        TrailingOffsetType trailing_offset_type,
        double offset,
        Price last,
    )
    cdef Price _calculate_new_trailing_price_bid_ask(
        self,
        Order order,
        TrailingOffsetType trailing_offset_type,
        double offset,
        Price bid,
        Price ask,
    )

# -- IDENTIFIER GENERATORS ------------------------------------------------------------------------

    cdef PositionId _get_position_id(self, Order order, bint generate=*)
    cdef PositionId _generate_venue_position_id(self)
    cdef VenueOrderId _generate_venue_order_id(self)
    cdef TradeId _generate_trade_id(self)

# -- EVENT HANDLING -------------------------------------------------------------------------------

    cdef void _accept_order(self, Order order) except *
    cdef void _update_order(self, Order order, Quantity qty, Price price=*, Price trigger_price=*, bint update_ocos=*) except *
    cdef void _update_oco_orders(self, Order order) except *
    cdef void _cancel_order(self, Order order, bint cancel_ocos=*) except *
    cdef void _cancel_oco_orders(self, Order order) except *
    cdef void _expire_order(self, Order order) except *

# -- EVENT GENERATORS -----------------------------------------------------------------------------

    cdef void _generate_order_rejected(self, Order order, str reason) except *
    cdef void _generate_order_accepted(self, Order order) except *
    cdef void _generate_order_pending_update(self, Order order) except *
    cdef void _generate_order_pending_cancel(self, Order order) except *
    cdef void _generate_order_modify_rejected(
        self,
        TraderId trader_id,
        StrategyId strategy_id,
        AccountId account_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
    ) except *
    cdef void _generate_order_cancel_rejected(
        self,
        TraderId trader_id,
        StrategyId strategy_id,
        AccountId account_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
    ) except *
    cdef void _generate_order_updated(self, Order order, Quantity qty, Price price, Price trigger_price) except *
    cdef void _generate_order_canceled(self, Order order) except *
    cdef void _generate_order_triggered(self, Order order) except *
    cdef void _generate_order_expired(self, Order order) except *
    cdef void _generate_order_filled(
        self,
        Order order,
        PositionId venue_position_id,
        Quantity last_qty,
        Price last_px,
        Currency quote_currency,
        Money commission,
        LiquiditySide liquidity_side
    ) except *
