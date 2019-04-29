#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="position.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False
from decimal import Decimal
from cpython.datetime cimport datetime
from typing import List

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
        self._order_ids = []          # type: List[OrderId]
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
        self.entry_direction = OrderSide.UNKNOWN
        self.entry_time = None
        self.exit_time = None
        self.average_entry_price = None
        self.average_exit_price = None
        self.points_realized = Decimal(0)
        self.return_realized = 0.0
        self.is_flat = True
        self.is_long = False
        self.is_short = False
        self.is_entered = False
        self.is_exited = False
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
        Return a list of all order identifiers.
        
        :return: List[OrderId]. 
        """
        return self._order_ids.copy()

    cpdef list get_execution_ids(self):
        """
        Return a list of all execution identifiers.
        
        :return: List[ExecutionId]. 
        """
        return self._execution_ids.copy()

    cpdef list get_execution_tickets(self):
        """
        Return a list of all execution tickets.
        
        :return: List[ExecutionTicket]. 
        """
        return self._execution_tickets.copy()

    cpdef list get_events(self):
        """
        Return a list of all order events.
        
        :return: List[Event].
        """
        return self._events.copy()

    cpdef int event_count(self):
        """
        Return the count of events applied to the position.
        
        :return: int.
        """
        return len(self._events)

    cpdef void apply(self, OrderEvent event):
        """
        Applies the given order event to the position. The given event type must
        be either OrderFilled or OrderPartiallyFilled.

        :param event: The order event to apply.
        """
        # Update events
        self._events.append(event)
        self.last_event = event

        # Update identifiers
        if event.order_id not in self._order_ids:
            self._order_ids.append(event.order_id)
        self._execution_ids.append(event.execution_id)
        self._execution_tickets.append(event.execution_ticket)
        self.last_order_id = event.order_id
        self.last_execution_id = event.execution_id
        self.last_execution_ticket = event.execution_ticket

        # Entry logic
        if not self.is_entered:
            self.from_order_id = event.order_id
            self.entry_direction = event.order_side
            self.entry_time = event.timestamp
            self.average_entry_price = event.average_price
            self.is_entered = True

        # Fill logic
        if event.order_side is OrderSide.BUY:
            if self.market_position == MarketPosition.SHORT:
                # Increment realized points and return of a short position
                self.points_realized += self._calculate_points(self.average_entry_price, event.average_price)
                self.return_realized += self._calculate_return(self.average_entry_price, event.average_price)
            self.relative_quantity += event.filled_quantity.value
        elif event.order_side is OrderSide.SELL:
            if self.market_position == MarketPosition.LONG:
                # Increment realized points and return of a long position
                self.points_realized += self._calculate_points(self.average_entry_price, event.average_price)
                self.return_realized += self._calculate_return(self.average_entry_price, event.average_price)
            self.relative_quantity -= event.filled_quantity.value

        self.quantity = Quantity(abs(self.relative_quantity))

        # Update peak quantity
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
            self.is_long = True
            self.is_flat = False
            self.is_short = False
        elif self.relative_quantity < 0:
            self.market_position = MarketPosition.SHORT
            self.is_short = True
            self.is_flat = False
            self.is_long = False
        else:
            self.market_position = MarketPosition.FLAT
            self.is_flat = True
            self.is_long = False
            self.is_short = False

        if self.is_exited and self.relative_quantity != 0:
            self.is_exited = False

    cpdef object points_unrealized(self, Price current_price):
        """
        Return the calculated unrealized points from the given current price.
         
        :param current_price: The current price of the position instrument.
        :return: Decimal.
        """
        if not self.is_entered or self.is_exited or self.entry_direction == OrderSide.UNKNOWN:
            return Decimal(0)
        return self._calculate_points(self.average_entry_price, current_price)

    cpdef float return_unrealized(self, Price current_price):
        """
        Return the calculated unrealized return from the given current price.
         
        :param current_price: The current price of the position instrument.
        :return: float.
        """
        if not self.is_entered or self.is_exited or self.market_position == MarketPosition.FLAT:
            return 0.0
        return self._calculate_return(self.average_entry_price, current_price)

    cdef object _calculate_points(self, Price entry_price, Price exit_price):
        """
        Return the calculated points from the given parameters.
        
        :return: Decimal.
        """
        if self.entry_direction == OrderSide.BUY:
            return exit_price.value - entry_price.value
        elif self.entry_direction == OrderSide.SELL:
            return entry_price.value - exit_price.value
        else:
            raise ValueError(f'Cannot calculate the points of a {self.entry_direction} direction.')

    cdef float _calculate_return(self, Price entry_price, Price exit_price):
        """
        Return the calculated return from the given parameters.
        
        :return: float.
        """
        if self.market_position == MarketPosition.LONG:
            return (exit_price.as_float() - entry_price.as_float()) / entry_price.as_float()
        elif self.market_position == MarketPosition.SHORT:
            return (entry_price.as_float() - exit_price.as_float()) / entry_price.as_float()
        else:
            raise ValueError(f'Cannot calculate the return of a {self.market_position} direction.')
