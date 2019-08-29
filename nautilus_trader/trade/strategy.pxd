# -------------------------------------------------------------------------------------------------
# <copyright file="strategy.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.common.account cimport Account
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.common.execution cimport ExecutionEngine
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.market_position cimport MarketPosition
from nautilus_trader.model.currency cimport ExchangeRateCalculator
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.identifiers cimport Symbol, TraderId, StrategyId, OrderId, PositionId
from nautilus_trader.model.generators cimport PositionIdGenerator
from nautilus_trader.model.objects cimport Price, Tick, BarType, Bar, Instrument
from nautilus_trader.model.order cimport Order, AtomicOrder, OrderFactory
from nautilus_trader.model.position cimport Position


cdef class TradingStrategy:
    cdef GuidFactory _guid_factory
    cdef readonly Clock clock
    cdef readonly LoggerAdapter log

    cdef readonly TraderId trader_id
    cdef readonly StrategyId id

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

    cdef readonly int tick_capacity
    cdef readonly int bar_capacity
    cdef dict _ticks
    cdef dict _bars
    cdef list _indicators
    cdef dict _indicator_updaters_ticks
    cdef dict _indicator_updaters_bars
    cdef ExchangeRateCalculator _exchange_calculator

    cdef readonly Account account
    cdef readonly Portfolio portfolio
    cdef DataClient _data_client
    cdef ExecutionEngine _exec_engine

    cdef readonly bint is_running

    cdef bint equals(self, TradingStrategy other)

#-- ABSTRACT METHODS ------------------------------------------------------------------------------#
    cpdef void on_start(self) except *
    cpdef void on_tick(self, Tick tick) except *
    cpdef void on_bar(self, BarType bar_type, Bar bar) except *
    cpdef void on_instrument(self, Instrument instrument) except *
    cpdef void on_event(self, Event event) except *
    cpdef void on_stop(self) except *
    cpdef void on_reset(self) except *
    cpdef void on_dispose(self) except *

#-- REGISTRATION METHODS --------------------------------------------------------------------------#
    cpdef void register_trader(self, TraderId trader_id) except *
    cpdef void register_data_client(self, DataClient client) except *
    cpdef void register_execution_engine(self, ExecutionEngine engine) except *
    cpdef void register_indicator_ticks(self, Symbol symbol, indicator, update_method) except *
    cpdef void register_indicator_bars(self, BarType bar_type, indicator, update_method) except *
    cpdef void register_entry_order(self, Order order, PositionId position_id) except *
    cpdef void register_stop_loss_order(self, Order order, PositionId position_id) except *
    cpdef void register_take_profit_order(self, Order order, PositionId position_id) except *

#-- HANDLER METHODS -------------------------------------------------------------------------------#
    cpdef void handle_tick(self, Tick tick)
    cpdef void handle_ticks(self, list ticks)
    cpdef void handle_bar(self, BarType bar_type, Bar bar)
    cpdef void handle_bars(self, BarType bar_type, list bars)
    cpdef void handle_instrument(self, Instrument instrument)
    cpdef void handle_event(self, Event event)

    cdef void _remove_atomic_child_orders(self, OrderId order_id)
    cdef void _remove_from_registered_orders(self, OrderId order_id)
    cdef void _process_modify_order_buffer(self, OrderId order_id)

#-- DATA METHODS ----------------------------------------------------------------------------------#
    cpdef datetime time_now(self)
    cpdef list instrument_symbols(self)
    cpdef Instrument get_instrument(self, Symbol symbol)
    cpdef dict instruments_all(self)
    cpdef void request_bars(self, BarType bar_type, datetime from_datetime=*, datetime to_datetime=*)
    cpdef void subscribe_ticks(self, Symbol symbol)
    cpdef void subscribe_bars(self, BarType bar_type)
    cpdef void subscribe_instrument(self, Symbol symbol)
    cpdef void unsubscribe_ticks(self, Symbol symbol)
    cpdef void unsubscribe_bars(self, BarType bar_type)
    cpdef void unsubscribe_instrument(self, Symbol symbol)
    cpdef bint has_ticks(self, Symbol symbol)
    cpdef bint has_bars(self, BarType bar_type)
    cpdef int tick_count(self, Symbol symbol)
    cpdef int bar_count(self, BarType bar_type)
    cpdef list ticks(self, Symbol symbol)
    cpdef list bars(self, BarType bar_type)
    cpdef Tick tick(self, Symbol symbol, int index)
    cpdef Bar bar(self, BarType bar_type, int index)

#-- INDICATOR METHODS -----------------------------------------------------------------------------#
    cpdef readonly list registered_indicators(self)
    cpdef readonly bint indicators_initialized(self)

#-- MANAGEMENT METHODS ----------------------------------------------------------------------------#
    cpdef OrderSide get_opposite_side(self, OrderSide side)
    cpdef OrderSide get_flatten_side(self, MarketPosition market_position)
    cpdef float get_exchange_rate(self, Currency quote_currency)

    cpdef Order order(self, OrderId order_id)
    cpdef dict orders(self)
    cpdef dict orders_working(self)
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
    cpdef dict positions(self)
    cpdef dict positions_open(self)
    cpdef dict positions_closed(self)
    cpdef bint position_exists(self, PositionId position_id)
    cpdef bint order_exists(self, OrderId order_id)
    cpdef bint is_order_working(self, OrderId order_id)
    cpdef bint is_order_complete(self, OrderId order_id)
    cpdef bint is_flat(self)
    cpdef int entry_orders_count(self)
    cpdef int stop_loss_orders_count(self)
    cpdef int take_profit_orders_count(self)

#-- COMMANDS --------------------------------------------------------------------------------------#
    cpdef void start(self)
    cpdef void stop(self)
    cpdef void reset(self)
    cpdef void dispose(self)
    cpdef void account_inquiry(self)
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

#-- BACKTEST METHODS ------------------------------------------------------------------------------#
    cpdef void change_clock(self, Clock clock)
    cpdef void change_guid_factory(self, GuidFactory guid_factory)
    cpdef void change_logger(self, Logger logger)
    cpdef void set_time(self, datetime time)
    cpdef dict iterate(self, datetime time)
