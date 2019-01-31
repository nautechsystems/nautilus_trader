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
from inv_trader.common.logger cimport LoggerAdapter
from inv_trader.model.identifiers cimport GUID, OrderId, PositionId
from inv_trader.model.events cimport Event
from inv_trader.model.position cimport Position


cdef class Portfolio:
    """
    Represents a trading portfolio of positions.
    """
    cdef Clock _clock
    cdef LoggerAdapter _log

    cdef dict _position_book
    cdef dict _order_p_index
    cdef list _registered_strategies
    cdef dict _positions_active
    cdef dict _positions_closed

    cpdef list registered_strategies(self)
    cpdef list registered_order_ids(self)
    cpdef list registered_position_ids(self)
    cpdef Position get_position(self, PositionId position_id)
    cpdef dict get_positions_all(self)
    cpdef dict get_positions_active_all(self)
    cpdef dict get_positions_closed_all(self)
    cpdef dict get_positions(self, GUID strategy_id)
    cpdef dict get_positions_active(self, GUID strategy_id)
    cpdef dict get_positions_closed(self, GUID strategy_id)
    cpdef bint is_strategy_flat(self, GUID strategy_id)
    cpdef bint is_flat(self)

    cpdef void register_strategy(self, GUID strategy_id)
    cpdef void register_order(self, OrderId order_id, PositionId position_id)
    cpdef void handle_event(self, Event event, GUID strategy_id)
