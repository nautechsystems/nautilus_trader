#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="identifiers.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from uuid import UUID
from inv_trader.core.precondition cimport Precondition


cdef class Identifier:
    """
    The abstract base class for all identifiers.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the Identifier abstract class.

        :param value: The value of the identifier.
        """
        Precondition.valid_string(value, 'value')

        self.value = value

    def __eq__(self, Identifier other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.value == other.value
        else:
            return False

    def __ne__(self, Identifier other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash(self.value)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the event.
        """
        return f"{self.value}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the event.
        """
        return f"<{str(self.__class__.__name__)}({self.value}) object at {id(self)}>"


cdef class GUID(Identifier):
    """
    Represents a globally unique identifier.
    """

    def __init__(self, value: UUID):
        """
        Initializes a new instance of the GUID class.

        :param value: The value of the GUID.
        """
        Precondition.type(value, UUID, 'value')

        super().__init__(str(value))


cdef class Label(Identifier):
    """
    Represents a valid label.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the Label class.

        :param value: The value of the label.
        """
        super().__init__(value)


cdef class AccountId(Identifier):
    """
    Represents a valid account identifier (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the AccountId class.

        :param value: The value of the account identifier.
        """
        super().__init__(value)


cdef class AccountNumber(Identifier):
    """
    Represents a valid account number (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the AccountNumber class.

        :param value: The value of the account number.
        """
        super().__init__(value)


cdef class OrderId(Identifier):
    """
    Represents a valid order identifier (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the OrderId class.

        :param value: The value of the order identifier.
        """
        super().__init__(value)


cdef class PositionId(Identifier):
    """
    Represents a valid position identifier (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the PositionId class.

        :param value: The value of the position identifier.
        """
        super().__init__(value)


cdef class ExecutionId(Identifier):
    """
    Represents a valid execution identifier (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the ExecutionId class.

        :param value: The value of the execution identifier.
        """
        super().__init__(value)


cdef class ExecutionTicket(Identifier):
    """
    Represents a valid execution ticket (should be unique).
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the ExecutionTicket class.

        :param value: The value of the execution ticket.
        """
        super().__init__(value)
