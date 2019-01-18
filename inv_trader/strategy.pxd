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

from inv_trader.core.decimal cimport Decimal
from inv_trader.common.clock cimport Clock
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.common.data cimport DataClient
from inv_trader.model.account cimport Account
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.enums.market_position cimport MarketPosition
from inv_trader.model.events cimport Event, AccountEvent, OrderEvent
from inv_trader.model.events cimport OrderFilled, OrderPartiallyFilled
from inv_trader.model.events cimport TimeEvent
from inv_trader.model.identifiers cimport GUID, Label, OrderId, PositionId
from inv_trader.model.objects cimport Symbol, Tick, BarType, Bar, Instrument
from inv_trader.model.order cimport Order
from inv_trader.model.order cimport OrderIdGenerator, OrderFactory
from inv_trader.model.position cimport Position


cdef class TradeStrategy:
    """
    The abstract base class for all trade strategies.
    """
    cdef Clock _clock
    cdef dict _timers
    cdef dict _ticks
    cdef dict _bars
    cdef dict _indicators
    cdef dict _indicator_updaters
    cdef OrderIdGenerator _order_id_generator

    cdef readonly LoggerAdapter log
    cdef readonly OrderFactory order_factory
    cdef readonly int bar_capacity
    cdef readonly bint is_running
    cdef readonly str name
    cdef readonly str label
    cdef readonly str order_id_tag
    cdef readonly GUID id
    cdef readonly dict _order_position_index
    cdef readonly dict _order_book
    cdef readonly dict _position_book
    cdef readonly DataClient _data_client
    cdef readonly ExecutionClient _exec_client
    cdef Account account

    cdef bint equals(self, TradeStrategy other)
# -- ABSTRACT METHODS ---------------------------------------------------------------------------- #
    cpdef void on_start(self)
    cpdef void on_tick(self, Tick tick)
    cpdef void on_bar(self, BarType bar_type, Bar bar)
    cpdef void on_event(self, Event event)
    cpdef void on_stop(self)
    cpdef void on_reset(self)

# -- DATA METHODS -------------------------------------------------------------------------------- #
    cpdef readonly datetime time_now(self)
    cpdef readonly list symbols(self)
    cpdef readonly list instruments(self)
    cpdef Instrument get_instrument(self, Symbol symbol)
    cpdef void historical_bars(self, BarType bar_type, int quantity)
    cpdef void historical_bars_from(self, BarType bar_type, datetime from_datetime)
    cpdef void subscribe_bars(self, BarType bar_type)
    cpdef void unsubscribe_bars(self, BarType bar_type)
    cpdef void subscribe_ticks(self, Symbol symbol)
    cpdef void unsubscribe_ticks(self, Symbol symbol)
    cpdef list bars(self, BarType bar_type)
    cpdef Bar bar(self, BarType bar_type, int index)
    cpdef Tick last_tick(self, Symbol symbol)

# -- INDICATOR METHODS --------------------------------------------------------------------------- #
    cpdef register_indicator(self, BarType bar_type, indicator, update_method)
    cpdef list indicators(self, BarType bar_type)
    cpdef readonly bint indicators_initialized(self, BarType bar_type)
    cpdef readonly bint all_indicators_initialized(self)

# -- ORDER MANAGEMENT METHODS -------------------------------------------------------------------- #
    cpdef OrderId generate_order_id(self, Symbol symbol)
    cpdef OrderSide get_opposite_side(self, OrderSide side)
    cpdef get_flatten_side(self, MarketPosition market_position)
    cpdef Order order(self, OrderId order_id)
    cpdef Position position(self, PositionId position_id)
    cpdef dict active_orders(self)
    cpdef dict active_positions(self)
    cpdef dict completed_orders(self)
    cpdef dict completed_positions(self)
    cpdef bint is_flat(self)

# -- COMMAND METHODS ----------------------------------------------------------------------------- #
    cpdef void start(self)
    cpdef void stop(self)
    cpdef void reset(self)
    cpdef set_time_alert(self, Label label, datetime alert_time)
    cpdef cancel_time_alert(self, Label label)
    cpdef set_timer(self, Label label, timedelta interval, datetime start_time, datetime stop_time, bint repeat)
    cpdef cancel_timer(self, Label label)
    cpdef collateral_inquiry(self)
    cpdef submit_order(self, Order order, PositionId position_id)
    cpdef modify_order(self, Order order, Decimal new_price)
    cpdef cancel_order(self, Order order, str cancel_reason)
    cpdef cancel_all_orders(self, str cancel_reason)
    cpdef flatten_position(self, PositionId position_id)
    cpdef flatten_all_positions(self)

# -- INTERNAL METHODS ---------------------------------------------------------------------------- #
    cpdef _register_data_client(self, DataClient client)
    cpdef _register_execution_client(self, ExecutionClient client)
    cpdef void _update_ticks(self, Tick tick)
    cpdef void _update_bars(self, BarType bar_type, Bar bar)
    cpdef void _update_indicators(self, BarType bar_type, Bar bar)
    cpdef void _update_events(self, Event event)
    cpdef void _change_clock(self, Clock clock)
    cpdef void _change_logger(self, Logger logger)
    cpdef void _set_time(self, datetime time)
    cpdef void _iterate(self, datetime time)
