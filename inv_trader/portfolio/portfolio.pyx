#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="portfolio.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime, timedelta
from collections import deque
from typing import Callable, Dict, List, Deque

from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.enums.market_position cimport MarketPosition
from inv_trader.common.clock cimport Clock, LiveClock
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.common.data cimport DataClient
from inv_trader.model.events cimport Event, OrderEvent
from inv_trader.model.events cimport OrderRejected, OrderCancelReject, OrderFilled, OrderPartiallyFilled
from inv_trader.model.identifiers cimport GUID, Label, OrderId, PositionId
from inv_trader.model.objects cimport Symbol, Price, Tick, BarType, Bar, Instrument
from inv_trader.model.order cimport Order, OrderIdGenerator, OrderFactory
from inv_trader.model.position cimport Position


cdef class Portfolio:
    """
    Represents a trading portfolio of positions.
    """

    def __init__(self,
                 Clock clock=LiveClock(),
                 Logger logger=None):
        """
        Initializes a new instance of the Portfolio class.
        """
        self._clock = clock
        if logger is None:
            self._log = LoggerAdapter(self.__class__.__name__)
        else:
            self._log = LoggerAdapter(self.__class__.__name__, logger)

        self._position_book = {}     # type: Dict[PositionId, Position]
        self._order_p_index = {}     # type: Dict[OrderId, PositionId]
        self._strategy_p_index = {}  # type: Dict[GUID, Dict[PositionId, Position]]
        self._active_positions = {}  # type: Dict[GUID, Dict[PositionId, Position]]
        self._closed_positions = {}  # type: Dict[GUID, Dict[PositionId, Position]]

    cpdef Position get_position(self, PositionId position_id):
        """
        TBA
        """
        Precondition.is_in(position_id, self._position_book, 'position_id', 'position_book')

        return self._position_book[position_id]

    cpdef dict get_positions_all(self):
        """
        :return: A copy of the list of all positions held by the portfolio.
        """
        return self._position_book.copy()

    cpdef dict get_active_positions_all(self):
        """
        :return: A copy of the list of all active positions held by the portfolio.
        """
        return self._active_positions.copy()

    cpdef dict get_closed_positions_all(self):
        """
        :return: A copy of the list of all closed positions held by the portfolio.
        """
        return self._closed_positions.copy()

    cpdef dict get_positions(self, GUID strategy_id):
        """
        Create and return a list of all positions associated with the strategy id.
        
        :param strategy_id: The strategy id associated with the positions.
        :return: The list of positions.
        """
        Precondition.is_in(strategy_id, self._strategy_p_index, 'strategy_id', 'strategy_p_index')

        return self._strategy_p_index[strategy_id].copy()

    cpdef dict get_active_positions(self, GUID strategy_id):
        """
        Create and return a list of all active positions associated with the strategy id.
        
        :param strategy_id: The strategy id associated with the positions.
        :return: The list of positions.
        """
        Precondition.is_in(strategy_id, self._strategy_p_index, 'strategy_id', 'strategy_p_index')

        return self._active_positions[strategy_id].copy()

    cpdef dict get_closed_positions(self, GUID strategy_id):
        """
        Create and return a list of all active positions associated with the strategy id.
        
        :param strategy_id: The strategy id associated with the positions.
        :return: The list of positions.
        """
        Precondition.is_in(strategy_id, self._strategy_p_index, 'strategy_id', 'strategy_p_index')

        return self._closed_positions[strategy_id].copy()

    cpdef bint is_strategy_flat(self, GUID strategy_id):
        """
        TBA
        :param strategy_id: 
        :return: 
        """
        return len(self._active_positions[strategy_id]) == 0

    cpdef bint is_flat(self):
        """
        TBA
        :return: 
        """
        for position in self._position_book.values():
            if not position.is_exited:
                return False

        return True

    cpdef void _register_strategy(self, GUID strategy_id):
        """
        TBA
        :param strategy_id: 
        :return: 
        """
        Precondition.not_in(strategy_id, self._strategy_p_index, 'strategy_id', 'strategy_p_index')
        Precondition.not_in(strategy_id, self._active_positions, 'strategy_id', 'active_positions')
        Precondition.not_in(strategy_id, self._closed_positions, 'strategy_id', 'closed_positions')

        self._strategy_p_index[strategy_id] = {}  # type: Dict[PositionId, Position]
        self._active_positions[strategy_id] = {}  # type: Dict[PositionId, Position]
        self._closed_positions[strategy_id] = {}  # type: Dict[PositionId, Position]

    cpdef void _register_order(self, OrderId order_id, PositionId position_id):
        """
        TBA
        """
        Precondition.not_in(order_id, self._order_p_index, 'order_id', 'order_position_index')

        self._order_p_index[order_id] = position_id

    cpdef void _on_event(self, Event event, GUID strategy_id):
        """
        TBA
        """
        Precondition.is_in(event.order_id, self._order_p_index, 'event.order_id', 'order_position_index')

        cdef PositionId position_id = self._order_p_index[event.order_id]
        cdef Position position

        # Position does not exist yet
        if position_id not in self._position_book:
            position = Position(
                event.symbol,
                position_id,
                event.execution_time)
            position.apply(event)

            # Add position to position book
            self._position_book[position_id] = position

            # Add position to active positions
            assert(position_id not in self._active_positions[strategy_id])
            self._active_positions[strategy_id][position_id] = position

            # Add position to strategy position index
            assert(position_id not in self._strategy_p_index[strategy_id])
            self._strategy_p_index[strategy_id][position_id] = position

            self._log.info(f"Opened {position}")

        # Position exists
        else:
            position = self._position_book[position_id]
            position.apply(event)

            if position.is_exited:
                self._log.info(f"Closed {position}")

                # Move to closed positions
                if position_id in self._active_positions[strategy_id]:
                    self._closed_positions[strategy_id][position_id] = position
                    del self._active_positions[strategy_id][position_id]
            else:
                # Check for overfill
                if position_id in self._closed_positions[strategy_id]:
                    self._active_positions[strategy_id][position_id] = position
                    del self._closed_positions[strategy_id][position_id]
                self._log.info(f"Modified {position}")
