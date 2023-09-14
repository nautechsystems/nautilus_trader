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

from libc.stdint cimport int64_t
from libc.stdint cimport uint32_t
from libc.stdint cimport uint64_t

from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.data cimport Data
from nautilus_trader.execution.matching_core cimport MatchingCore
from nautilus_trader.execution.messages cimport BatchCancelOrders
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.book cimport BookOrder
from nautilus_trader.model.data.book cimport OrderBookDelta
from nautilus_trader.model.data.book cimport OrderBookDeltas
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport BookType
from nautilus_trader.model.enums_c cimport LiquiditySide
from nautilus_trader.model.enums_c cimport MarketStatus
from nautilus_trader.model.enums_c cimport OmsType
from nautilus_trader.model.enums_c cimport TimeInForce
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
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.limit_if_touched cimport LimitIfTouchedOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.market_if_touched cimport MarketIfTouchedOrder
from nautilus_trader.model.orders.market_to_limit cimport MarketToLimitOrder
from nautilus_trader.model.orders.stop_limit cimport StopLimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder
from nautilus_trader.model.orders.trailing_stop_limit cimport TrailingStopLimitOrder
from nautilus_trader.model.orders.trailing_stop_market cimport TrailingStopMarketOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class OrderMatchingEngine:
    cdef Clock _clock
    cdef LoggerAdapter _log
    cdef MessageBus _msgbus
    cdef OrderBook _book
    cdef OrderBook _opening_auction_book
    cdef OrderBook _closing_auction_book
    cdef FillModel _fill_model
    # cdef object _auction_match_algo
    cdef bint _bar_execution
    cdef bint _reject_stop_orders
    cdef bint _support_gtd_orders
    cdef bint _use_position_ids
    cdef bint _use_random_ids
    cdef bint _use_reduce_only
    cdef dict _account_ids

    cdef readonly Venue venue
    """The venue for the matching engine.\n\n:returns: `Venue`"""
    cdef readonly Instrument instrument
    """The instrument for the matching engine.\n\n:returns: `Instrument`"""
    cdef readonly uint32_t raw_id
    """The instruments raw integer ID for the exchange.\n\n:returns: `int`"""
    cdef readonly BookType book_type
    """The order book type for the matching engine.\n\n:returns: `BookType`"""
    cdef readonly OmsType oms_type
    """The order management system type for the matching engine.\n\n:returns: `OmsType`"""
    cdef readonly MarketStatus market_status
    """The market status for the matching engine.\n\n:returns: `MarketStatus`"""
    cdef readonly CacheFacade cache
    """The cache for the matching engine.\n\n:returns: `CacheFacade`"""
    cdef readonly MessageBus msgbus
    """The message bus for the matching engine.\n\n:returns: `MessageBus`"""

    cdef MatchingCore _core
    cdef bint _has_targets
    cdef int64_t _target_bid
    cdef int64_t _target_ask
    cdef int64_t _target_last
    cdef Bar _last_bid_bar
    cdef Bar _last_ask_bar

    cdef int _position_count
    cdef int _order_count
    cdef int _execution_count

    cpdef void reset(self)
    cpdef void set_fill_model(self, FillModel fill_model)

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Price best_bid_price(self)
    cpdef Price best_ask_price(self)
    cpdef OrderBook get_book(self)
    cpdef list get_open_orders(self)
    cpdef list get_open_bid_orders(self)
    cpdef list get_open_ask_orders(self)
    cpdef bint order_exists(self, ClientOrderId client_order_id)

# -- DATA PROCESSING ------------------------------------------------------------------------------

    cpdef void process_order_book_delta(self, OrderBookDelta delta)
    cpdef void process_order_book_deltas(self, OrderBookDeltas deltas)
    cpdef void process_quote_tick(self, QuoteTick tick)
    cpdef void process_trade_tick(self, TradeTick tick)
    cpdef void process_bar(self, Bar bar)
    cpdef void process_status(self, MarketStatus status)
    cpdef void process_auction_book(self, OrderBook book)
    cdef void _process_trade_ticks_from_bar(self, Bar bar)
    cdef void _process_quote_ticks_from_bar(self)

# -- TRADING COMMANDS -----------------------------------------------------------------------------

    cpdef void process_order(self, Order order, AccountId account_id)
    cpdef void process_modify(self, ModifyOrder command, AccountId account_id)
    cpdef void process_cancel(self, CancelOrder command, AccountId account_id)
    cpdef void process_cancel_all(self, CancelAllOrders command, AccountId account_id)
    cpdef void process_batch_cancel(self, BatchCancelOrders command, AccountId account_id)
    cdef void _process_market_order(self, MarketOrder order)
    cdef void _process_market_to_limit_order(self, MarketToLimitOrder order)
    cdef void _process_limit_order(self, LimitOrder order)
    cdef void _process_stop_market_order(self, StopMarketOrder order)
    cdef void _process_stop_limit_order(self, StopLimitOrder order)
    cdef void _process_market_if_touched_order(self, MarketIfTouchedOrder order)
    cdef void _process_limit_if_touched_order(self, LimitIfTouchedOrder order)
    cdef void _process_trailing_stop_market_order(self, TrailingStopMarketOrder order)
    cdef void _process_trailing_stop_limit_order(self, TrailingStopLimitOrder order)
    cdef void _process_auction_market_order(self, MarketOrder order)
    cdef void _process_auction_limit_order(self, LimitOrder order)
    cdef void _process_auction_book_order(self, BookOrder order, TimeInForce time_in_force)
    cdef void _update_limit_order(self, Order order, Quantity qty, Price price)
    cdef void _update_stop_market_order(self, StopMarketOrder order, Quantity qty, Price trigger_price)
    cdef void _update_stop_limit_order(self, StopLimitOrder order, Quantity qty, Price price, Price trigger_price)
    cdef void _update_market_if_touched_order(self, Order order, Quantity qty, Price trigger_price)
    cdef void _update_limit_if_touched_order(self, Order order, Quantity qty, Price price, Price trigger_price)
    cdef void _update_trailing_stop_order(self, Order order)

# -- ORDER PROCESSING -----------------------------------------------------------------------------

    cpdef void iterate(self, uint64_t timestamp_ns)
    cpdef list determine_limit_price_and_volume(self, Order order)
    cpdef list determine_market_price_and_volume(self, Order order)
    cpdef void fill_market_order(self, Order order)
    cpdef void fill_limit_order(self, Order order)

    cpdef void apply_fills(
        self,
        Order order,
        list fills,
        LiquiditySide liquidity_side,
        PositionId venue_position_id=*,
        Position position=*,
    )
    cpdef void fill_order(
        self,
        Order order,
        Price last_px,
        Quantity last_qty,
        LiquiditySide liquidity_side,
        PositionId venue_position_id=*,
        Position position=*,
    )

# -- IDENTIFIER GENERATORS ------------------------------------------------------------------------

    cdef PositionId _get_position_id(self, Order order, bint generate=*)
    cdef PositionId _generate_venue_position_id(self)
    cdef VenueOrderId _generate_venue_order_id(self)
    cdef TradeId _generate_trade_id(self)
    cdef str _generate_trade_id_str(self)

# -- EVENT HANDLING -------------------------------------------------------------------------------

    cpdef void accept_order(self, Order order)
    cpdef void expire_order(self, Order order)
    cpdef void cancel_order(self, Order order, bint cancel_contingencies=*)
    cpdef void update_order(self, Order order, Quantity qty, Price price=*, Price trigger_price=*, bint update_contingencies=*)
    cpdef void trigger_stop_order(self, Order order)
    cdef void _cancel_contingent_orders(self, Order order)
    cdef void _update_contingent_orders(self, Order order)

# -- EVENT GENERATORS -----------------------------------------------------------------------------

    cdef void _generate_order_rejected(self, Order order, str reason)
    cdef void _generate_order_accepted(self, Order order)
    cdef void _generate_order_modify_rejected(
        self,
        TraderId trader_id,
        StrategyId strategy_id,
        AccountId account_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
    )
    cdef void _generate_order_cancel_rejected(
        self,
        TraderId trader_id,
        StrategyId strategy_id,
        AccountId account_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
    )
    cpdef void _generate_order_updated(self, Order order, Quantity qty, Price price, Price trigger_price)
    cdef void _generate_order_canceled(self, Order order)
    cdef void _generate_order_triggered(self, Order order)
    cdef void _generate_order_expired(self, Order order)
    cdef void _generate_order_filled(
        self,
        Order order,
        PositionId venue_position_id,
        Quantity last_qty,
        Price last_px,
        Currency quote_currency,
        Money commission,
        LiquiditySide liquidity_side
    )
