#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="portfolio.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from inv_trader.common.clock cimport Clock
from inv_trader.common.logger cimport LoggerAdapter
from inv_trader.model.identifiers cimport GUID
from inv_trader.model.events cimport Event


cdef class Portfolio:
    """
    Represents a trading portfolio of positions.
    """
    cdef Clock _clock
    cdef LoggerAdapter _log

    cdef dict _position_book
    cdef dict _strategy_position_index
    cdef dict _active_positions
    cdef dict _closed_positions

    cpdef list get_position_ids_all(self)
    cpdef dict get_positions_all(self)
    cpdef dict get_active_positions_all(self)
    cpdef dict get_closed_positions_all(self)
    cpdef list get_positions(self, GUID strategy_id)
    cpdef list get_active_positions(self, GUID strategy_id)
    cpdef list get_closed_positions(self, GUID strategy_id)

    cpdef void _register_strategy(self, GUID strategy_id)
    cpdef void _on_event(self, Event event, GUID strategy_id)
