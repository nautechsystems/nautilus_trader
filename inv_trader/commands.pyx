#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="commands.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from datetime import datetime
from decimal import Decimal
from uuid import UUID

from inv_trader.core.precondition cimport Precondition
from inv_trader.model.order import Order


cdef class Command:
    """
    The abstract base class for all commands.
    """
    cdef object _command_id
    cdef object _command_timestamp

    def __init__(self,
                 identifier: UUID,
                 timestamp: datetime):
        """
        Initializes a new instance of the Command abstract class.

        :param identifier: The commands identifier.
        :param timestamp: The commands timestamp.
        """
        Precondition.type(identifier, UUID, 'identifier')
        Precondition.type(timestamp, datetime, 'timestamp')

        self._command_id = identifier
        self._command_timestamp = timestamp

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

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash(self._command_id)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the command.
        """
        return f"{self.__class__.__name__}()"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the command.
        """
        return f"<{str(self)} object at {id(self)}>"

    @property
    def id(self) -> UUID:
        """
        :return: The commands identifier.
        """
        return self._command_id

    @property
    def timestamp(self) -> datetime:
        """
        :return: The commands timestamp (the time the command was created).
        """
        return self._command_timestamp


cdef class OrderCommand(Command):
    """
    The abstract base class for all order commands.
    """
    cdef object _order

    def __init__(self,
                 order: Order,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the OrderCommand abstract class.

        :param order: The commands order.
        :param command_id: The commands identifier.
        :param command_timestamp: The order commands timestamp.
        """
        Precondition.type(order, Order, 'order')
        Precondition.type(command_id, UUID, 'command_id')
        Precondition.type(command_timestamp, datetime, 'command_timestamp')

        super().__init__(command_id, command_timestamp)
        self._order = order

    def __str__(self) -> str:
        """
        :return: The str() string representation of the command.
        """
        return f"{self.__class__.__name__}({self._order})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the command.
        """
        return f"<{str(self)} object at {id(self)}>"

    @property
    def order(self) -> Order:
        """
        :return: The commands order.
        """
        return self._order


cdef class SubmitOrder(OrderCommand):
    """
    Represents a command to submit the given order.
    """

    def __init__(self,
                 order: Order,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the SubmitOrder class.

        :param order: The commands order to submit.
        :param command_id: The commands identifier.
        :param command_timestamp: The commands timestamp.
        """
        Precondition.type(order, Order, 'order')
        Precondition.type(command_id, UUID, 'command_id')
        Precondition.type(command_timestamp, datetime, 'command_timestamp')

        super().__init__(
            order,
            command_id,
            command_timestamp)


cdef class CancelOrder(OrderCommand):
    """
    Represents a command to cancel the given order.
    """
    cdef str _cancel_reason

    def __init__(self,
                 order: Order,
                 cancel_reason: str,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the CancelOrder class.

        :param order: The commands order to cancel.
        :param cancel_reason: The reason for cancellation.
        :param command_id: The commands identifier.
        :param command_timestamp: The commands timestamp.
        """
        Precondition.type(order, Order, 'order')
        Precondition.type(command_id, UUID, 'command_id')
        Precondition.type(command_timestamp, datetime, 'command_timestamp')
        Precondition.valid_string(cancel_reason, 'cancel_reason')

        super().__init__(
            order,
            command_id,
            command_timestamp)

        self._cancel_reason = cancel_reason

    @property
    def cancel_reason(self) -> str:
        """
        :return: The commands order cancel reason.
        """
        return self._cancel_reason


cdef class ModifyOrder(OrderCommand):
    """
    Represents a command to modify the given order with the given modified price.
    """
    cdef object _modified_price

    def __init__(self,
                 order: Order,
                 modified_price: Decimal,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the ModifyOrder class.

        :param order: The commands order to modify.
        :param modified_price: The commands modified price for the order.
        :param command_id: The commands identifier.
        :param command_timestamp: The commands timestamp.
        """
        Precondition.type(order, Order, 'order')
        Precondition.type(modified_price, Decimal, 'modified_price')
        Precondition.type(command_id, UUID, 'command_id')
        Precondition.type(command_timestamp, datetime, 'command_timestamp')
        Precondition.positive(modified_price, 'modified_price')

        super().__init__(
            order,
            command_id,
            command_timestamp)

        self._modified_price = modified_price

    @property
    def modified_price(self) -> Decimal:
        """
        :return: The commands modified price for the order.
        """
        return self._modified_price


cdef class CollateralInquiry(Command):
    """
    Represents a request for a FIX collateral inquiry of all connected accounts.
    """

    def __init__(self,
                 command_id: UUID,
                 command_timestamp: datetime):
        """
        Initializes a new instance of the CollateralInquiry class.

        :param command_id: The commands identifier.
        :param command_timestamp: The order commands timestamp.
        """
        Precondition.type(command_id, UUID, 'command_id')
        Precondition.type(command_timestamp, datetime, 'command_timestamp')

        super().__init__(command_id, command_timestamp)
