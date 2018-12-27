#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="position.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from datetime import datetime
from decimal import Decimal
from typing import List

from inv_trader.core.precondition cimport Precondition
from inv_trader.model.enums import MarketPosition, OrderSide
from inv_trader.model.objects import Symbol
from inv_trader.model.events import OrderEvent
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
from inv_trader.model.identifiers import PositionId, ExecutionId, ExecutionTicket


cdef class Position:
    """
    Represents a position in a financial market.
    """
    cdef int _relative_quantity
    cdef int _peak_quantity
    cdef list _events

    cdef readonly object symbol
    cdef readonly object id
    cdef readonly object timestamp
    cdef readonly object entry_time
    cdef readonly object exit_time
    cdef readonly object average_entry_price
    cdef readonly object average_exit_price
    cdef readonly list execution_ids
    cdef readonly list execution_tickets

    def __init__(self,
                 symbol: Symbol,
                 position_id: PositionId,
                 timestamp: datetime):
        """
        Initializes a new instance of the Position class.

        :param symbol: The orders symbol.
        :param position_id: The positions identifier.
        :param timestamp: The positions initialization timestamp.
        :raises ValueError: If the position_id is not a valid string.
        """
        Precondition.type(symbol, Symbol, 'symbol')
        Precondition.type(position_id, PositionId, 'position_id')
        Precondition.type(timestamp, datetime, 'timestamp')

        self.symbol = symbol
        self.id = position_id
        self.timestamp = timestamp
        self.entry_time = None
        self.exit_time = None
        self.average_entry_price = None
        self.average_exit_price = None
        self._relative_quantity = 0
        self._peak_quantity = 0
        self._events = []              # type: List[OrderEvent]
        self.execution_ids = []        # type: List[ExecutionId]
        self.execution_tickets = []    # type: List[ExecutionTicket]

    cpdef int quantity(self):
        """
        :return: The positions quantity.
        """
        return abs(self._relative_quantity)

    cpdef bint is_entered(self):
        """
        :return: A value indicating whether the position has entered into the market (bool).
        """
        return self.entry_time is not None

    cpdef bint is_exited(self):
        """
        :return: A value indicating whether the position has exited from the market (bool).
        """
        return self.exit_time is not None

    cpdef object market_position(self):
        """
        :return: The positions current market position (MarketPosition).
        """
        if self._relative_quantity > 0:
            return MarketPosition.LONG
        elif self._relative_quantity < 0:
            return MarketPosition.SHORT
        else:
            return MarketPosition.FLAT

    cpdef int event_count(self):
        """
        :return: The count of events since the position was initialized.
        """
        return len(self._events)

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.id == other.id
        else:
            return False

    def __ne__(self, other):
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the position.
        """
        cdef str quantity = '{:,}'.format(self.quantity())
        return (f"Position(id={self.id}) "
                f"{self.symbol} {self.market_position().name} {quantity}")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the position.
        """
        cdef object attrs = vars(self)
        cdef str props = ', '.join("%s=%s" % item for item in attrs.items()).replace(', _', ', ')
        return f"<{self.__class__.__name__}({props[1:]}) object at {id(self)}>"

    cpdef void apply(self, event: OrderEvent):
        """
        Applies the given order event to the position.

        :param event: The order event to apply.
        """
        Precondition.type(event, OrderEvent, 'event')

        self._events.append(event)

        # Handle event
        if isinstance(event, OrderFilled):
            self._update_position(
                event.order_side,
                event.filled_quantity,
                event.average_price,
                event.execution_time)
            self.execution_ids.append(event.execution_id)
            self.execution_tickets.append(event.execution_ticket)
        elif isinstance(event, OrderPartiallyFilled):
            self._update_position(
                event.order_side,
                event.filled_quantity,
                event.average_price,
                event.execution_time)
            self.execution_ids.append(event.execution_id)
            self.execution_tickets.append(event.execution_ticket)
        else:
            raise TypeError("Cannot apply event (unrecognized event).")

    cdef void _update_position(
            self,
            order_side: OrderSide,
            int quantity,
            average_price: Decimal,
            event_time: datetime):
        Precondition.type(order_side, OrderSide, 'order_side')
        Precondition.type(average_price, Decimal, 'average_price')
        Precondition.type(event_time, datetime, 'event_time')
        Precondition.positive(quantity, 'quantity')
        Precondition.positive(average_price, 'average_price')

        if order_side is OrderSide.BUY:
            self._relative_quantity += quantity
        elif order_side is OrderSide.SELL:
            self._relative_quantity -= quantity

        # Update the peak quantity
        if abs(self._relative_quantity) > self._peak_quantity:
            self._peak_quantity = abs(self._relative_quantity)

        # Capture the first time of entry
        if self.entry_time is None:
            self.entry_time = event_time

        self.average_entry_price = average_price

        # Position was exited
        if self.is_entered() and self._relative_quantity == 0:
            self.exit_time = event_time
            self.average_exit_price = average_price
