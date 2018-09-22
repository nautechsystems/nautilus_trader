#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="identifiers.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc

from inv_trader.core.precondition import Precondition


class Identifier:
    """
    The abstract base class for all identifiers.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self, value: str):
        """
        Initializes a new instance of the Identifier abstract class.

        :param value: The value of the identifier.
        """
        Precondition.valid_string(value, 'value')

        self._value = value

    @property
    def value(self) -> str:
        """
        :return: The identifiers value.
        """
        return self._value

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self._value == other._value
        else:
            return False

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash(self._value)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the event.
        """
        return f"{self._value}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the event.
        """
        return f"<{str(self.__class__.__name__)}({self._value}) object at {id(self)}>"


class Label(Identifier):
    """
    Represents a valid label.
    """

    def __init__(self, value: str):
        """
        Initializes a new instance of the OrderId class.

        :param value: The value of the order identifier.
        """
        super().__init__(value)


class OrderId(Identifier):
    """
    Represents a valid order identifier (should be unique).
    """

    def __init__(self, value: str):
        """
        Initializes a new instance of the OrderId class.

        :param value: The value of the order identifier.
        """
        super().__init__(value)


class PositionId(Identifier):
    """
    Represents a valid position identifier (should be unique).
    """

    def __init__(self, value: str):
        """
        Initializes a new instance of the PositionId class.

        :param value: The value of the position identifier.
        """
        super().__init__(value)


class ExecutionId(Identifier):
    """
    Represents a valid execution identifier (should be unique).
    """

    def __init__(self, value: str):
        """
        Initializes a new instance of the ExecutionId class.

        :param value: The value of the execution identifier.
        """
        super().__init__(value)


class ExecutionTicket(Identifier):
    """
    Represents a valid execution ticket (should be unique).
    """

    def __init__(self, value: str):
        """
        Initializes a new instance of the ExecutionTicket class.

        :param value: The value of the execution ticket.
        """
        super().__init__(value)
