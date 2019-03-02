#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from cpython.datetime cimport datetime, timedelta

from inv_trader.common.account cimport Account
from inv_trader.common.clock cimport Clock
from inv_trader.common.guid cimport GuidFactory
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.common.data cimport DataClient
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.enums.market_position cimport MarketPosition
from inv_trader.model.events cimport Event
from inv_trader.model.identifiers cimport GUID, Label, OrderId, PositionId
from inv_trader.model.identifiers cimport PositionIdGenerator
from inv_trader.model.objects cimport ValidString, Symbol, Price, Tick, BarType, Bar, Instrument
from inv_trader.model.order cimport Order, AtomicOrder, OrderFactory
from inv_trader.model.position cimport Position
from inv_trader.portfolio.portfolio cimport Portfolio


cdef class TradeStrategy:
    """
    The abstract base class for all trade strategies.
    """
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef dict _timers
    cdef dict _ticks
    cdef dict _bars
    cdef dict _indicators
    cdef dict _indicator_updaters
    cdef Account account
    cdef Portfolio _portfolio

    cdef readonly LoggerAdapter log
    cdef readonly OrderFactory order_factory
    cdef readonly PositionIdGenerator position_id_generator
    cdef readonly int bar_capacity
    cdef readonly bint is_running
    cdef readonly Label name
    cdef readonly ValidString id_tag_trader
    cdef readonly ValidString id_tag_strategy
    cdef readonly GUID id
    cdef readonly DataClient _data_client
    cdef readonly ExecutionClient _exec_client

    cdef bint equals(self, TradeStrategy other)

# -- ABSTRACT METHODS ---------------------------------------------------------------------------- #
    cpdef void on_start(self)
    cpdef void on_tick(self, Tick tick)
    cpdef void on_bar(self, BarType bar_type, Bar bar)
    cpdef void on_event(self, Event event)
    cpdef void on_stop(self)
    cpdef void on_reset(self)

# -- REGISTRATION AND HANDLER METHODS ------------------------------------------------------------ #
    cpdef void register_data_client(self, DataClient client)
    cpdef void register_execution_client(self, ExecutionClient client)
    cpdef void handle_tick(self, Tick tick)
    cpdef void handle_bar(self, BarType bar_type, Bar bar)
    cpdef void handle_event(self, Event event)

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
    cpdef void register_indicator(self, BarType bar_type, indicator, update_method)
    cpdef list indicators(self, BarType bar_type)
    cpdef readonly bint indicators_initialized(self, BarType bar_type)
    cpdef readonly bint indicators_initialized_all(self)

# -- MANAGEMENT METHODS -------------------------------------------------------------------------- #
    cpdef PositionId generate_position_id(self, Symbol symbol)
    cpdef OrderSide get_opposite_side(self, OrderSide side)
    cpdef OrderSide get_flatten_side(self, MarketPosition market_position)
    cpdef bint order_exists(self, OrderId order_id)
    cpdef bint order_active(self, OrderId order_id)
    cpdef bint order_complete(self, OrderId order_id)
    cpdef Order order(self, OrderId order_id)
    cpdef dict orders_all(self)
    cpdef dict orders_active(self)
    cpdef dict orders_completed(self)
    cpdef bint position_exists(self, PositionId position_id)
    cpdef Position position(self, PositionId position_id)
    cpdef dict positions_all(self)
    cpdef dict positions_active(self)
    cpdef dict positions_closed(self)
    cpdef bint is_flat(self)

# -- COMMAND METHODS ----------------------------------------------------------------------------- #
    cpdef void start(self)
    cpdef void stop(self)
    cpdef void reset(self)
    cpdef void collateral_inquiry(self)
    cpdef void submit_order(self, Order order, PositionId position_id)
    cpdef void submit_atomic_order(self, AtomicOrder order, PositionId position_id)
    cpdef void modify_order(self, Order order, Price new_price)
    cpdef void cancel_order(self, Order order, str cancel_reason=*)
    cpdef void cancel_all_orders(self, str cancel_reason=*)
    cpdef void flatten_position(self, PositionId position_id)
    cpdef void flatten_all_positions(self)
    cpdef void set_time_alert(self, Label label, datetime alert_time)
    cpdef void cancel_time_alert(self, Label label)
    cpdef void set_timer(self, Label label, timedelta interval, datetime start_time, datetime stop_time, bint repeat)
    cpdef void cancel_timer(self, Label label)

# -- BACKTEST METHODS ---------------------------------------------------------------------------- #
    cpdef void change_clock(self, Clock clock)
    cpdef void change_guid_factory(self, GuidFactory guid_factory)
    cpdef void change_logger(self, Logger logger)
    cpdef void set_time(self, datetime time)
    cpdef void iterate(self, datetime time)
