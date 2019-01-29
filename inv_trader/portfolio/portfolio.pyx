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

        self._position_book = {}            # type: Dict[PositionId, Position]
        self._strategy_position_index = {}  # type: Dict[GUID, List[PositionId]]
        self._active_positions = {}         # type: Dict[GUID, Dict[Symbol, Position]]
        self._closed_positions = {}         # type: Dict[GUID, List[Position]]

    cpdef list get_position_ids_all(self):
        """
        :return: A copy of the portfolios strategy position index.
        """
        return [self._position_book.keys()]

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

    cpdef list get_positions(self, GUID strategy_id):
        """
        Create and return a list of all positions associated with the strategy id.
        
        :param strategy_id: The strategy id associated with the positions.
        :return: The list of positions.
        """
        Precondition.is_in(strategy_id, self._strategy_position_index, 'strategy_id', 'strategy_position_index')

        return self._position_book[strategy_id]

    cpdef list get_active_positions(self, GUID strategy_id):
        """
        Create and return a list of all active positions associated with the strategy id.
        
        :param strategy_id: The strategy id associated with the positions.
        :return: The list of positions.
        """
        Precondition.is_in(strategy_id, self._strategy_position_index, 'strategy_id', 'strategy_position_index')

        return self._active_positions[strategy_id]

    cpdef list get_closed_positions(self, GUID strategy_id):
        """
        Create and return a list of all active positions associated with the strategy id.
        
        :param strategy_id: The strategy id associated with the positions.
        :return: The list of positions.
        """
        Precondition.is_in(strategy_id, self._strategy_position_index, 'strategy_id', 'strategy_position_index')

        return self._closed_positions[strategy_id]

    cpdef void _register_strategy(self, GUID strategy_id):
        """
        TBA
        :param strategy_id: 
        :return: 
        """
        Precondition.not_in(strategy_id, self._strategy_position_index, 'strategy_id', 'strategy_position_index')

        self._strategy_position_index[strategy_id] = []
        self._active_positions[strategy_id] = {}
        self._closed_positions[strategy_id] = []

    cpdef void _on_event(self, Event event, GUID strategy_id):
        """
        TBA
        """
        cdef Position position

        if event.symbol not in self._active_positions[strategy_id]:
            position = Position(
                event.symbol,
                event.order_id,
                event.execution_time)
            position.apply(event)
            self._position_book[position.id] = position

            self._strategy_position_index[strategy_id].append(position.id)
            self._active_positions[strategy_id][event.symbol] = position
            self._log.info(f"Opened {position}")
        else:
            position = self._active_positions[strategy_id][event.symbol]
            position.apply(event)

            if position.is_exited:
                self._log.info(f"Closed {position}")
                self._closed_positions[strategy_id].append(position)
                del self._active_positions[strategy_id][event.symbol]
            else:
                self._log.info(f"Modified {position}")
