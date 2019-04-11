#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="portfolio.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.common.clock cimport Clock
from inv_trader.common.guid cimport GuidFactory
from inv_trader.common.logger cimport LoggerAdapter
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.model.identifiers cimport GUID, OrderId, PositionId
from inv_trader.model.events cimport AccountEvent, OrderEvent
from inv_trader.model.objects cimport Money
from inv_trader.model.position cimport Position
from inv_trader.strategy cimport TradeStrategy
from inv_trader.portfolio.analyzer cimport Analyzer


cdef class Portfolio:
    """
    Represents a trading portfolio of positions.
    """
    cdef LoggerAdapter _log
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef ExecutionClient _exec_client
    cdef dict _position_book
    cdef dict _order_p_index
    cdef list _registered_strategies
    cdef dict _positions_active
    cdef dict _positions_closed
    cdef Money _account_capital
    cdef bint _account_initialized

    cdef readonly list position_opened_events
    cdef readonly list position_closed_events
    cdef readonly Analyzer analyzer

    cpdef list registered_strategies(self)
    cpdef list registered_order_ids(self)
    cpdef list registered_position_ids(self)
    cpdef bint position_exists(self, PositionId position_id)
    cpdef bint position_active(self, PositionId position_id)
    cpdef bint position_closed(self, PositionId position_id)
    cpdef bint order_has_position(self, OrderId order_id)
    cpdef Position get_position_for_order(self, OrderId order_id)
    cpdef Position get_position(self, PositionId position_id)
    cpdef dict get_positions_all(self)
    cpdef dict get_positions_active_all(self)
    cpdef dict get_positions_closed_all(self)
    cpdef dict get_positions(self, GUID strategy_id)
    cpdef dict get_positions_active(self, GUID strategy_id)
    cpdef dict get_positions_closed(self, GUID strategy_id)
    cpdef bint is_strategy_flat(self, GUID strategy_id)
    cpdef bint is_flat(self)
    cpdef int positions_count(self)
    cpdef int positions_active_count(self)
    cpdef int positions_closed_count(self)

    cpdef void register_execution_client(self, ExecutionClient client)
    cpdef void register_strategy(self, TradeStrategy strategy)
    cpdef void register_order(self, OrderId order_id, PositionId position_id)
    cpdef void handle_order_fill(self, OrderEvent event, GUID strategy_id)
    cpdef void handle_transaction(self, AccountEvent event)
    cpdef void reset(self)

    cdef void _position_opened(self, Position position, GUID strategy_id)
    cdef void _position_modified(self, Position position, GUID strategy_id)
    cdef void _position_closed(self, Position position, GUID strategy_id)
