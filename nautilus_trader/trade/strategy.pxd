# -------------------------------------------------------------------------------------------------
# <copyright file="strategy.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.common.account cimport Account
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.common.execution cimport ExecutionClient
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.market_position cimport MarketPosition
from nautilus_trader.model.currency cimport ExchangeRateCalculator
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.identifiers cimport TraderId, StrategyId, OrderId, PositionId
from nautilus_trader.model.identifiers cimport PositionIdGenerator
from nautilus_trader.model.objects cimport ValidString, Symbol, Price, Tick, BarType, Bar, Instrument
from nautilus_trader.model.order cimport Order, AtomicOrder, OrderFactory
from nautilus_trader.model.position cimport Position
from nautilus_trader.trade.portfolio cimport Portfolio


cdef class TradeStrategy:
    """
    The base class for all trade strategies.
    """
    cdef GuidFactory _guid_factory
    cdef readonly Clock clock
    cdef readonly LoggerAdapter log

    cdef readonly TraderId trader_id
    cdef readonly StrategyId id
    cdef readonly ValidString id_tag_trader
    cdef readonly ValidString id_tag_strategy

    cdef readonly bint flatten_on_sl_reject
    cdef readonly bint flatten_on_stop
    cdef readonly bint cancel_all_orders_on_stop
    cdef readonly OrderFactory order_factory
    cdef readonly PositionIdGenerator position_id_generator
    cdef dict _entry_orders
    cdef dict _stop_loss_orders
    cdef dict _take_profit_orders
    cdef dict _atomic_order_ids
    cdef dict _modify_order_buffer

    cdef readonly int bar_capacity
    cdef dict _timers
    cdef dict _ticks
    cdef dict _bars
    cdef dict _indicators
    cdef dict _indicator_updaters
    cdef ExchangeRateCalculator _exchange_calculator

    cdef readonly Account account
    cdef DataClient _data_client
    cdef ExecutionClient _exec_client
    cdef Portfolio _portfolio

    cdef readonly bint is_data_client_registered
    cdef readonly bint is_exec_client_registered
    cdef readonly bint is_portfolio_registered
    cdef readonly bint is_running

    cdef bint equals(self, TradeStrategy other)

# -- ABSTRACT METHODS ---------------------------------------------------------------------------- #
    cpdef on_start(self)
    cpdef on_tick(self, Tick tick)
    cpdef on_bar(self, BarType bar_type, Bar bar)
    cpdef on_event(self, Event event)
    cpdef on_stop(self)
    cpdef on_reset(self)
    cpdef on_dispose(self)

# -- REGISTRATION METHODS ------------------------------------------------------------------------ #
    cpdef void register_trader_id(self, TraderId trader_id, ValidString order_id_tag_trader)
    cpdef void register_data_client(self, DataClient client)
    cpdef void register_execution_client(self, ExecutionClient client)
    cpdef void register_indicator(self, BarType bar_type, indicator, update_method)
    cpdef void register_entry_order(self, Order order, PositionId position_id)
    cpdef void register_stop_loss_order(self, Order order, PositionId position_id)
    cpdef void register_take_profit_order(self, Order order, PositionId position_id)

# -- HANDLER METHODS ----------------------------------------------------------------------------- #
    cpdef void handle_tick(self, Tick tick)
    cpdef void handle_bar(self, BarType bar_type, Bar bar)
    cpdef void handle_event(self, Event event)

    cdef void _remove_atomic_child_orders(self, OrderId order_id)
    cdef void _remove_from_registered_orders(self, OrderId order_id)
    cdef void _process_modify_order_buffer(self, OrderId order_id)

# -- DATA METHODS -------------------------------------------------------------------------------- #
    cpdef readonly datetime time_now(self)
    cpdef readonly list symbols(self)
    cpdef readonly list instruments(self)
    cpdef Instrument get_instrument(self, Symbol symbol)
    cpdef void historical_bars(self, BarType bar_type, int quantity=*)
    cpdef void historical_bars_from(self, BarType bar_type, datetime from_datetime)
    cpdef void subscribe_bars(self, BarType bar_type)
    cpdef void unsubscribe_bars(self, BarType bar_type)
    cpdef void subscribe_ticks(self, Symbol symbol)
    cpdef void unsubscribe_ticks(self, Symbol symbol)
    cpdef list bars(self, BarType bar_type)
    cpdef Bar bar(self, BarType bar_type, int index)
    cpdef Bar last_bar(self, BarType bar_type)
    cpdef Tick last_tick(self, Symbol symbol)

# -- INDICATOR METHODS --------------------------------------------------------------------------- #
    cpdef list indicators(self, BarType bar_type)
    cpdef readonly bint indicators_initialized(self, BarType bar_type)
    cpdef readonly bint indicators_initialized_all(self)

# -- MANAGEMENT METHODS -------------------------------------------------------------------------- #
    cpdef OrderSide get_opposite_side(self, OrderSide side)
    cpdef OrderSide get_flatten_side(self, MarketPosition market_position)
    cpdef float get_exchange_rate(self, Currency quote_currency)
    cpdef Order order(self, OrderId order_id)
    cpdef dict orders_all(self)
    cpdef dict orders_active(self)
    cpdef dict orders_completed(self)
    cpdef dict entry_orders(self)
    cpdef dict stop_loss_orders(self)
    cpdef dict take_profit_orders(self)
    cpdef list entry_order_ids(self)
    cpdef list stop_loss_order_ids(self)
    cpdef list take_profit_order_ids(self)
    cpdef Order entry_order(self, OrderId order_id)
    cpdef Order stop_loss_order(self, OrderId order_id)
    cpdef Order take_profit_order(self, OrderId order_id)
    cpdef Position position(self, PositionId position_id)
    cpdef dict positions_all(self)
    cpdef dict positions_active(self)
    cpdef dict positions_closed(self)
    cpdef bint is_position_exists(self, PositionId position_id)
    cpdef bint is_order_exists(self, OrderId order_id)
    cpdef bint is_order_active(self, OrderId order_id)
    cpdef bint is_order_complete(self, OrderId order_id)
    cpdef bint is_flat(self)
    cpdef int entry_orders_count(self)
    cpdef int stop_loss_orders_count(self)
    cpdef int take_profit_orders_count(self)

# -- COMMAND METHODS ----------------------------------------------------------------------------- #
    cpdef void start(self)
    cpdef void stop(self)
    cpdef void reset(self)
    cpdef void dispose(self)
    cpdef void collateral_inquiry(self)
    cpdef void submit_order(self, Order order, PositionId position_id)
    cpdef void submit_entry_order(self, Order order, PositionId position_id)
    cpdef void submit_stop_loss_order(self, Order order, PositionId position_id)
    cpdef void submit_take_profit_order(self, Order order, PositionId position_id)
    cpdef void submit_atomic_order(self, AtomicOrder atomic_order, PositionId position_id)
    cpdef void modify_order(self, Order order, Price new_price)
    cpdef void cancel_order(self, Order order, str cancel_reason=*)
    cpdef void cancel_all_orders(self, str cancel_reason=*)
    cpdef void flatten_position(self, PositionId position_id)
    cpdef void flatten_all_positions(self)

# -- BACKTEST METHODS ---------------------------------------------------------------------------- #
    cpdef void change_clock(self, Clock clock)
    cpdef void change_guid_factory(self, GuidFactory guid_factory)
    cpdef void change_logger(self, Logger logger)
    cpdef void set_time(self, datetime time)
    cpdef dict iterate(self, datetime time)
