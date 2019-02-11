#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="commands.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.model.objects cimport ValidString, Price
from inv_trader.model.identifiers cimport GUID, Label, PositionId
from inv_trader.model.order cimport Order, AtomicOrder


cdef class Command:
    """
    The abstract base class for all commands.
    """

    def __init__(self,
                 GUID identifier,
                 datetime timestamp):
        """
        Initializes a new instance of the Command abstract class.

        :param identifier: The commands identifier.
        :param timestamp: The commands timestamp.
        """
        self.id = identifier
        self.timestamp = timestamp

    def __eq__(self, Command other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.id == other.id
        else:
            return False

    def __ne__(self, Command other):
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash(self.id)

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


cdef class CollateralInquiry(Command):
    """
    Represents a request for a FIX collateral inquiry of all connected accounts.
    """

    def __init__(self,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the CollateralInquiry class.

        :param command_id: The commands identifier.
        :param command_timestamp: The order commands timestamp.
        """
        super().__init__(command_id, command_timestamp)


cdef class OrderCommand(Command):
    """
    The abstract base class for all order commands.
    """

    def __init__(self,
                 Order order,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the OrderCommand abstract class.

        :param order: The commands order.
        :param command_id: The commands identifier.
        :param command_timestamp: The commands timestamp.
        """
        super().__init__(command_id, command_timestamp)
        self.order = order

    def __str__(self) -> str:
        """
        :return: The str() string representation of the command.
        """
        return f"{self.__class__.__name__}({self.order})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the command.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class SubmitOrder(OrderCommand):
    """
    Represents a command to submit the given order.
    """

    def __init__(self,
                 Order order,
                 PositionId position_id,
                 GUID strategy_id,
                 Label strategy_name,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the SubmitOrder class.

        :param order: The commands order to submit.
        :param position_id: The command position identifier.
        :param strategy_id: The strategy identifier to associate with the order.
        :param strategy_name: The name of the strategy associated with the order.
        :param command_id: The commands identifier.
        :param command_timestamp: The commands timestamp.
        """
        super().__init__(order,
                         command_id,
                         command_timestamp)
        self.position_id = position_id
        self.strategy_id = strategy_id
        self.strategy_name = strategy_name


cdef class SubmitAtomicOrder(Command):
    """
    Represents a command to submit an atomic order consisting of parent and child orders.
    """

    def __init__(self,
                 AtomicOrder atomic_order,
                 PositionId position_id,
                 GUID strategy_id,
                 Label strategy_name,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the SubmitOrder class.

        :param atomic_order: The commands atomic order to submit.
        :param position_id: The command position identifier.
        :param strategy_id: The strategy identifier to associate with the order.
        :param strategy_name: The name of the strategy associated with the order.
        :param command_id: The commands identifier.
        :param command_timestamp: The commands timestamp.
        """
        super().__init__(command_id,
                         command_timestamp)
        self.atomic_order = atomic_order
        self.position_id = position_id
        self.strategy_id = strategy_id
        self.strategy_name = strategy_name

    def __str__(self) -> str:
        """
        :return: The str() string representation of the command.
        """
        return f"{self.__class__.__name__}({self.atomic_order})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the command.
        """
        return f"<{str(self)} object at {id(self)}>"

cdef class ModifyOrder(OrderCommand):
    """
    Represents a command to modify the given order with the given modified price.
    """

    def __init__(self,
                 Order order,
                 Price modified_price,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the ModifyOrder class.

        :param order: The commands order to modify.
        :param modified_price: The commands modified price for the order.
        :param command_id: The commands identifier.
        :param command_timestamp: The commands timestamp.
        """
        super().__init__(order,
                         command_id,
                         command_timestamp)
        self.modified_price = modified_price


cdef class CancelOrder(OrderCommand):
    """
    Represents a command to cancel the given order.
    """

    def __init__(self,
                 Order order,
                 ValidString cancel_reason,
                 GUID command_id,
                 datetime command_timestamp):
        """
        Initializes a new instance of the CancelOrder class.

        :param order: The commands order to cancel.
        :param cancel_reason: The reason for cancellation.
        :param command_id: The commands identifier.
        :param command_timestamp: The commands timestamp.
        """
        super().__init__(
            order,
            command_id,
            command_timestamp)
        self.cancel_reason = cancel_reason
