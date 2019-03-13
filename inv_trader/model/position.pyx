#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="position.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime
from typing import Set, List

from inv_trader.enums.market_position cimport MarketPosition, market_position_string
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.model.objects cimport Symbol
from inv_trader.model.events cimport OrderEvent
from inv_trader.model.identifiers cimport PositionId, ExecutionId, ExecutionTicket


cdef class Position:
    """
    Represents a position in a financial market.
    """

    def __init__(self,
                 Symbol symbol,
                 PositionId position_id,
                 datetime timestamp):
        """
        Initializes a new instance of the Position class.

        :param symbol: The positions symbol.
        :param position_id: The positions identifier.
        :param timestamp: The positions initialization timestamp.
        """
        self._order_ids = set()       # type: Set[OrderId]
        self._execution_ids = []      # type: List[ExecutionId]
        self._execution_tickets = []  # type: List[ExecutionTicket]
        self._events = []             # type: List[OrderEvent]

        self.symbol = symbol
        self.id = position_id
        self.from_order_id = None
        self.last_order_id = None
        self.last_execution_id = None
        self.last_execution_ticket = None
        self.relative_quantity = 0
        self.quantity = Quantity(0)
        self.peak_quantity = Quantity(0)
        self.market_position = MarketPosition.FLAT
        self.timestamp = timestamp
        self.entry_time = None
        self.exit_time = None
        self.average_entry_price = None
        self.average_exit_price = None
        self.return_realized = 0.0
        self.is_entered = False
        self.is_exited = False
        self.event_count = 0
        self.last_event = None

    cdef bint equals(self, Position other):
        """
        Compare if the object equals the given object.
        
        :param other: The other object to compare
        :return: True if the objects are equal, otherwise False.
        """
        return self.id.equals(other.id)

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        return self.equals(other)

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.equals(other)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the position.
        """
        return f"Position(id={self.id.value}) {self.status_string()}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the position.
        """
        return f"<{str(self)} object at {id(self)}>"

    cdef str status_string(self):
        """
        Return the positions status as a string.

        :return: str.
        """
        cdef str quantity = '' if self.relative_quantity == 0 else ' {:,}'.format(self.quantity.value)
        return f"{self.symbol} {market_position_string(self.market_position)}{quantity}"

    cpdef list get_order_ids(self):
        """
        Return a copy of the list of internally held from order identifiers.
        
        :return: List[OrderId]. 
        """
        return list(self._order_ids.copy())

    cpdef list get_execution_ids(self):
        """
        Return A copy of the list of internally held execution ids.
        
        :return: List[ExecutionId]. 
        """
        return self._execution_ids.copy()

    cpdef list get_execution_tickets(self):
        """
        Return a copy of the list of internally held execution tickets.
        
        :return: List[ExecutionTicket]. 
        """
        return self._execution_tickets.copy()

    cpdef list get_events(self):
        """
        Return a copy of the list of internally held events.
        
        :return: List[Event].
        """
        return self._events.copy()

    cpdef void apply(self, OrderEvent event):
        """
        Applies the given order event to the position. The given event type must
        be either OrderFilled or OrderPartiallyFilled.

        :param event: The order event to apply.
        """
        self._events.append(event)
        self.event_count += 1
        self.last_event = event

        # Handle event
        self._order_ids.add(event.order_id)
        self._execution_ids.append(event.execution_id)
        self._execution_tickets.append(event.execution_ticket)
        self.last_order_id = event.order_id
        self.last_execution_id = event.execution_id
        self.last_execution_ticket = event.execution_ticket

        # Entry logic
        if not self.is_entered:
            self.from_order_id = event.order_id
            self.entry_time = event.timestamp
            self.average_entry_price = event.average_price
            self.is_entered = True

        # Fill logic
        if event.order_side is OrderSide.BUY:
            if self.market_position == MarketPosition.SHORT:
                # Increment realized return of a short position
                self.return_realized += self._calculate_return(
                    MarketPosition.SHORT,
                    self.average_entry_price,
                    event.average_price)
            self.relative_quantity += event.filled_quantity.value
        elif event.order_side is OrderSide.SELL:
            if self.market_position == MarketPosition.LONG:
                # Increment realized return of a long position
                self.return_realized += self._calculate_return(
                    MarketPosition.LONG,
                    self.average_entry_price,
                    event.average_price)
            self.relative_quantity -= event.filled_quantity.value

        self.quantity = Quantity(abs(self.relative_quantity))

        if self.quantity > self.peak_quantity:
            self.peak_quantity = self.quantity

        # Exit logic
        if self.relative_quantity == 0:
            self.exit_time = event.timestamp
            self.average_exit_price = event.average_price
            self.is_exited = True

        # Market position logic
        if self.relative_quantity > 0:
            self.market_position = MarketPosition.LONG
        elif self.relative_quantity < 0:
            self.market_position = MarketPosition.SHORT
        else:
            self.market_position = MarketPosition.FLAT

        # Check overfill
        if self.is_exited and self.relative_quantity != 0:
            self.is_exited = False

    cpdef float return_unrealized(self, Price current_price):
        """
        Return the calculated unrealized return percentage from the given current price.
         
        :param current_price: The current price of the position instrument.
        :return: float.
        """
        if not self.is_entered or self.is_exited or self.market_position is MarketPosition.FLAT:
            return 0.0
        return self._calculate_return(self.market_position,
                                      self.average_entry_price,
                                      current_price)

    cdef float _calculate_return(
            self,
            MarketPosition direction,
            Price entry_price,
            Price exit_price):
        """
        Return the calculated return from the given parameters.
        """
        if direction is MarketPosition.LONG:
            return (exit_price.as_float() - entry_price.as_float()) / exit_price.as_float()
        if direction is MarketPosition.SHORT:
            return (entry_price.as_float() - exit_price.as_float()) / exit_price.as_float()
        else:
            raise ValueError(f'Cannot calculate the return of a {direction} direction.')
