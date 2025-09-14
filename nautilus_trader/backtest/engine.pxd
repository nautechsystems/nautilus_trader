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
from libc.stdint cimport uint32_t
from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.margin_models cimport MarginModel
from nautilus_trader.backtest.execution_client cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FeeModel
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.models cimport LatencyModel
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.backtest cimport TimeEventAccumulatorAPI
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport AggressorSide
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport MarketStatus
from nautilus_trader.core.rust.model cimport MarketStatusAction
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport PriceRaw
from nautilus_trader.core.rust.model cimport TimeInForce
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.data.messages cimport DataCommand
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport RequestData
from nautilus_trader.data.messages cimport SubscribeData
from nautilus_trader.data.messages cimport UnsubscribeData
from nautilus_trader.execution.matching_core cimport MatchingCore
from nautilus_trader.execution.messages cimport BatchCancelOrders
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BookOrder
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDeltas
from nautilus_trader.model.data cimport OrderBookDepth10
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
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
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.limit_if_touched cimport LimitIfTouchedOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.market_if_touched cimport MarketIfTouchedOrder
from nautilus_trader.model.orders.market_to_limit cimport MarketToLimitOrder
from nautilus_trader.model.orders.stop_limit cimport StopLimitOrder
from nautilus_trader.model.orders.stop_market cimport StopMarketOrder
from nautilus_trader.model.position cimport Position


cdef class BacktestEngine:
    cdef object _config
    cdef Clock _clock
    cdef Logger _log
    cdef TimeEventAccumulatorAPI _accumulator

    cdef object _kernel
    cdef UUID4 _instance_id
    cdef DataEngine _data_engine
    cdef str _run_config_id
    cdef UUID4 _run_id
    cdef datetime _run_started
    cdef datetime _run_finished
    cdef datetime _backtest_start
    cdef datetime _backtest_end

    cdef dict[Venue, SimulatedExchange] _venues
    cdef set[InstrumentId] _has_data
    cdef set[InstrumentId] _has_book_data
    cdef list[Data] _data
    cdef uint64_t _data_len
    cdef uint64_t _index
    cdef uint64_t _iteration
    cdef object _data_iterator
    cdef uint64_t _last_ns
    cdef uint64_t _end_ns
    cdef dict[str, RequestData] _data_requests
    cdef set[str] _backtest_subscription_names
    cdef dict[str, uint64_t] _last_subscription_ts
    cdef list _response_data

    cdef CVec _advance_time(self, uint64_t ts_now)
    cdef void _process_raw_time_event_handlers(
        self,
        CVec raw_handlers,
        uint64_t ts_now,
        bint only_now,
        bint as_of_now=*,
    )

    cpdef void _handle_data_command(self, DataCommand command)
    cdef void _handle_subscribe(self, SubscribeData command)
    cpdef void _update_subscription_data(self, str subscription_name, uint64_t start_time, uint64_t end_time)
    cpdef void _handle_data_response(self, DataResponse response)
    cpdef void _handle_unsubscribe(self, UnsubscribeData command)


cdef inline bint should_skip_time_event(
    uint64_t ts_event_init,
    uint64_t ts_now,
    bint only_now,
    bint as_of_now,
):
    if only_now and ts_event_init < ts_now:
        return True
    if (not only_now) and (ts_event_init == ts_now):
        return True
    if as_of_now and ts_event_init > ts_now:
        return True

    return False


cdef class BacktestDataIterator:
    cdef object _empty_data_callback
    cdef Logger _log
    cdef dict[str, list[Data]] _data
    cdef dict[str, str] _data_name
    cdef dict[str, str] _data_priority
    cdef dict[str, int] _data_len
    cdef dict[str, int] _data_index
    cdef list[tuple[uint64_t, str, int]] _heap
    cdef int _next_data_priority
    cdef list[Data] _single_data
    cdef str _single_data_name
    cdef int _single_data_priority
    cdef int _single_data_len
    cdef int _single_data_index
    cdef bint _is_single_data
    cdef dict _data_update_function

    cdef dict[str, object] _stream_iterators
    cdef dict[str, uint64_t] _stream_current_window_start
    cdef dict[str, bint] _stream_exhausted
    cdef dict[str, bint] _stream_append_data
    cdef dict[str, uint64_t] _stream_chunk_duration_ns

    cpdef void _reset_single_data(self)
    cdef void _add_data(self, str data_name, list data_list, bint append_data=*)
    cpdef void remove_data(self, str data_name, bint complete_remove=*)
    cpdef void _activate_single_data(self)
    cpdef void _deactivate_single_data(self)
    cpdef Data next(self)
    cpdef void _push_data(self, int data_priority, int data_index)
    cpdef void _update_data(self, int data_priority)
    cpdef void _reset_heap(self)
    cpdef void set_index(self, str data_name, int index)
    cpdef bint is_done(self)
    cpdef dict all_data(self)
    cpdef list[Data] data(self, str data_name)


cdef class SimulatedExchange:
    cdef Clock _clock
    cdef Logger _log

    cdef readonly Venue id
    """The exchange ID.\n\n:returns: `Venue`"""
    cdef readonly OmsType oms_type
    """The exchange order management system type.\n\n:returns: `OmsType`"""
    cdef readonly BookType book_type
    """The exchange default order book type.\n\n:returns: `BookType`"""
    cdef readonly MessageBus msgbus
    """The message bus wired to the exchange.\n\n:returns: `MessageBus`"""
    cdef readonly Cache cache
    """The cache wired to the exchange.\n\n:returns: `CacheFacade`"""
    cdef readonly BacktestExecClient exec_client
    """The execution client wired to the exchange.\n\n:returns: `BacktestExecClient`"""

    cdef readonly AccountType account_type
    """The account base currency.\n\n:returns: `AccountType`"""
    cdef readonly Currency base_currency
    """The account base currency (None for multi-currency accounts).\n\n:returns: `Currency` or ``None``"""
    cdef readonly list starting_balances
    """The account starting balances for each backtest run.\n\n:returns: `bool`"""
    cdef readonly default_leverage
    """The accounts default leverage.\n\n:returns: `Decimal`"""
    cdef readonly dict leverages
    """The accounts instrument specific leverage configuration.\n\n:returns: `dict[InstrumentId, Decimal]`"""
    cdef readonly MarginModel margin_model
    """The margin calculation model for the exchange.\n\n:returns: `MarginModel`"""
    cdef readonly bint is_frozen_account
    """If the account for the exchange is frozen.\n\n:returns: `bool`"""
    cdef readonly LatencyModel latency_model
    """The latency model for the exchange.\n\n:returns: `LatencyModel`"""
    cdef readonly FillModel fill_model
    """The fill model for the exchange.\n\n:returns: `FillModel`"""
    cdef readonly FeeModel fee_model
    """The fee model for the exchange.\n\n:returns: `FeeModel`"""
    cdef readonly bint reject_stop_orders
    """If stop orders are rejected on submission if in the market.\n\n:returns: `bool`"""
    cdef readonly bint support_gtd_orders
    """If orders with GTD time in force will be supported by the venue.\n\n:returns: `bool`"""
    cdef readonly bint support_contingent_orders
    """If contingent orders will be supported/respected by the venue.\n\n:returns: `bool`"""
    cdef readonly bint use_position_ids
    """If venue position IDs will be generated on order fills.\n\n:returns: `bool`"""
    cdef readonly bint use_random_ids
    """If venue order and position IDs will be randomly generated UUID4s.\n\n:returns: `bool`"""
    cdef readonly bint use_reduce_only
    """If the `reduce_only` option on orders will be honored.\n\n:returns: `bool`"""
    cdef readonly bint use_message_queue
    """If an internal message queue is being used to sequentially process incoming trading commands.\n\n:returns: `bool`"""
    cdef readonly bint bar_execution
    """If bars should be processed by the matching engine(s) (and move the market).\n\n:returns: `bool`"""
    cdef readonly bint bar_adaptive_high_low_ordering
    """If the processing order of bar prices is adaptive based on a heuristic.\n\n:returns: `bool`"""
    cdef readonly bint trade_execution
    """If trades should be processed by the matching engine(s) (and move the market).\n\n:returns: `bool`"""
    cdef readonly list modules
    """The simulation modules registered with the exchange.\n\n:returns: `list[SimulationModule]`"""
    cdef readonly dict instruments
    """The exchange instruments.\n\n:returns: `dict[InstrumentId, Instrument]`"""

    cdef dict _matching_engines
    cdef object _message_queue
    cdef list _inflight_queue
    cdef dict _inflight_counter

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register_client(self, BacktestExecClient client)
    cpdef void set_fill_model(self, FillModel fill_model)
    cpdef void set_latency_model(self, LatencyModel latency_model)
    cpdef void initialize_account(self)
    cpdef void add_instrument(self, Instrument instrument)

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Price best_bid_price(self, InstrumentId instrument_id)
    cpdef Price best_ask_price(self, InstrumentId instrument_id)
    cpdef OrderBook get_book(self, InstrumentId instrument_id)
    cpdef OrderMatchingEngine get_matching_engine(self, InstrumentId instrument_id)
    cpdef dict get_matching_engines(self)
    cpdef dict get_books(self)
    cpdef list get_open_orders(self, InstrumentId instrument_id=*)
    cpdef list get_open_bid_orders(self, InstrumentId instrument_id=*)
    cpdef list get_open_ask_orders(self, InstrumentId instrument_id=*)
    cpdef Account get_account(self)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void adjust_account(self, Money adjustment)
    cpdef void update_instrument(self, Instrument instrument)
    cdef tuple generate_inflight_command(self, TradingCommand command)
    cpdef void send(self, TradingCommand command)
    cpdef void process_order_book_delta(self, OrderBookDelta delta)
    cpdef void process_order_book_deltas(self, OrderBookDeltas deltas)
    cpdef void process_order_book_depth10(self, OrderBookDepth10 depth)
    cpdef void process_quote_tick(self, QuoteTick tick)
    cpdef void process_trade_tick(self, TradeTick tick)
    cpdef void process_bar(self, Bar bar)
    cpdef void process_instrument_close(self, InstrumentClose close)
    cpdef void process_instrument_status(self, InstrumentStatus data)
    cpdef void process(self, uint64_t ts_now)
    cpdef void reset(self)

    cdef void _process_trading_command(self, TradingCommand command)
    cdef void _process_modify_submitted_order(self, ModifyOrder command)
    cdef void _generate_order_modify_rejected(
        self,
        TraderId trader_id,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
        AccountId account_id,
    )
    cdef void _generate_order_updated(
        self,
        Order order,
        Quantity quantity,
        Price price,
        Price trigger_price,
    )

# -- EVENT GENERATORS -----------------------------------------------------------------------------

    cdef void _generate_fresh_account_state(self)


cdef class OrderMatchingEngine:
    cdef Clock _clock
    cdef Logger _log
    cdef MessageBus _msgbus
    cdef OrderBook _book
    cdef OrderBook _opening_auction_book
    cdef OrderBook _closing_auction_book
    cdef FillModel _fill_model
    cdef FeeModel _fee_model
    cdef InstrumentClose _instrument_close
    # cdef object _auction_match_algo
    cdef bint _instrument_has_expiration
    cdef bint _reject_stop_orders
    cdef bint _support_gtd_orders
    cdef bint _support_contingent_orders
    cdef bint _use_position_ids
    cdef bint _use_random_ids
    cdef bint _use_reduce_only
    cdef bint _bar_execution
    cdef bint _bar_adaptive_high_low_ordering
    cdef bint _trade_execution
    cdef dict _account_ids
    cdef dict _execution_bar_types
    cdef dict _execution_bar_deltas
    cdef dict _cached_filled_qty

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
    cdef readonly AccountType account_type
    """The account type for the matching engine.\n\n:returns: `AccountType`"""
    cdef readonly MarketStatus market_status
    """The market status for the matching engine.\n\n:returns: `MarketStatus`"""
    cdef readonly CacheFacade cache
    """The cache for the matching engine.\n\n:returns: `CacheFacade`"""
    cdef readonly MessageBus msgbus
    """The message bus for the matching engine.\n\n:returns: `MessageBus`"""

    cdef MatchingCore _core
    cdef bint _has_targets
    cdef PriceRaw _target_bid
    cdef PriceRaw _target_ask
    cdef PriceRaw _target_last
    cdef Bar _last_bid_bar
    cdef Bar _last_ask_bar

    cdef int _position_count
    cdef int _order_count
    cdef int _execution_count

    cpdef void reset(self)
    cpdef void set_fill_model(self, FillModel fill_model)
    cpdef void update_instrument(self, Instrument instrument)

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
    cpdef void process_order_book_depth10(self, OrderBookDepth10 depth)
    cpdef void process_quote_tick(self, QuoteTick tick)
    cpdef void process_trade_tick(self, TradeTick tick)
    cpdef void process_bar(self, Bar bar)
    cpdef void process_status(self, MarketStatusAction status)
    cpdef void process_auction_book(self, OrderBook book)
    cpdef void process_instrument_close(self, InstrumentClose close)
    cdef void _process_trade_ticks_from_bar(self, Bar bar)
    cdef TradeTick _create_base_trade_tick(self, Bar bar, Quantity size)
    cdef void _process_trade_bar_open(self, Bar bar, TradeTick tick)
    cdef void _process_trade_bar_high(self, Bar bar, TradeTick tick)
    cdef void _process_trade_bar_low(self, Bar bar, TradeTick tick)
    cdef void _process_trade_bar_close(self, Bar bar, TradeTick tick)
    cdef void _process_quote_ticks_from_bar(self)
    cdef QuoteTick _create_base_quote_tick(self, Quantity bid_size, Quantity ask_size)
    cdef void _process_quote_bar_open(self, QuoteTick tick)
    cdef void _process_quote_bar_high(self, QuoteTick tick)
    cdef void _process_quote_bar_low(self, QuoteTick tick)
    cdef void _process_quote_bar_close(self, QuoteTick tick)

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
    cdef void _process_trailing_stop_order(self, Order order)
    cdef void _process_auction_market_order(self, MarketOrder order)
    cdef void _process_auction_limit_order(self, LimitOrder order)
    cdef void _process_auction_book_order(self, BookOrder order, TimeInForce time_in_force)
    cdef void _update_limit_order(self, Order order, Quantity qty, Price price)
    cdef void _update_stop_market_order(self, Order order, Quantity qty, Price trigger_price)
    cdef void _update_stop_limit_order(self, Order order, Quantity qty, Price price, Price trigger_price)
    cdef void _update_market_if_touched_order(self, Order order, Quantity qty, Price trigger_price)
    cdef void _update_limit_if_touched_order(self, Order order, Quantity qty, Price price, Price trigger_price)
    cdef void _update_trailing_stop_market_order(self, Order order, Quantity qty, Price trigger_price)
    cdef void _update_trailing_stop_limit_order(self, Order order, Quantity qty, Price price, Price trigger_price)

# -- ORDER PROCESSING -----------------------------------------------------------------------------

    cpdef void iterate(self, uint64_t timestamp_ns, AggressorSide aggressor_side=*)
    cpdef list determine_limit_price_and_volume(self, Order order)
    cpdef list determine_market_price_and_volume(self, Order order)
    cdef list determine_market_fills_with_simulation(self, Order order)
    cdef list determine_limit_fills_with_simulation(self, Order order)
    cpdef void fill_market_order(self, Order order)
    cpdef void fill_limit_order(self, Order order)
    cdef void _trail_stop_order(self, Order order)

    cpdef void apply_fills(
        self,
        Order order,
        list fills,
        LiquiditySide liquidity_side,
        PositionId venue_position_id=*,
        Position position=*,
    )
    cdef void _generate_spread_leg_fills(
        self,
        Order order,
        list fills,
        LiquiditySide liquidity_side,
    )
    cdef dict _calculate_leg_execution_prices(
        self,
        list leg_tuples,
        Price spread_execution_price,
        Quantity spread_quantity,
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

    cdef VenueOrderId _get_venue_order_id(self, Order order)
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

    cdef void _generate_order_rejected(self, Order order, str reason, bint due_post_only=*)
    cdef void _generate_order_accepted(self, Order order, VenueOrderId venue_order_id)
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
    cdef void _generate_order_canceled(self, Order order, VenueOrderId venue_order_id)
    cdef void _generate_order_triggered(self, Order order)
    cdef void _generate_order_expired(self, Order order)
    cdef void _generate_order_filled(
        self,
        Order order,
        VenueOrderId venue_order_id,
        PositionId venue_position_id,
        Quantity last_qty,
        Price last_px,
        Currency quote_currency,
        Money commission,
        LiquiditySide liquidity_side
    )
