#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="identifiers.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from uuid import UUID
from typing import Dict

from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport Clock, LiveClock
from inv_trader.model.objects cimport ValidString, Symbol


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

    cpdef bint equals(self, Identifier other):
        """
        Compare if the object equals the given object.
        
        :param other: The other object to compare
        :return: True if the objects are equal, otherwise False.
        """
        if isinstance(other, self.__class__):
            return self.value == other.value
        else:
            return False

    def __eq__(self, Identifier other) -> bool:
        """
        Override the default equality comparison.
        """
        return self.equals(other)

    def __ne__(self, Identifier other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash(self.value)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the identifier.
        """
        return f"{str(self.__class__.__name__)}({self.value})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the identifier.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class GUID(Identifier):
    """
    Represents a globally unique identifier.
    """

    def __init__(self, value: UUID):
        """
        Initializes a new instance of the GUID class.

        :param value: The value of the GUID.
        """
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


cdef str SEPARATOR = '-'


cdef class IdentifierGenerator:
    """
    Provides a generator for unique identifier strings.
    """

    def __init__(self,
                 ValidString id_tag_trader,
                 ValidString id_tag_strategy,
                 Clock clock):
        """
        Initializes a new instance of the IdentifierGenerator class.

        :param id_tag_trader: The identifier tag for the trader.
        :param id_tag_strategy: The identifier tag for the strategy.
        :param clock: The internal clock.
        :raises ValueError: If the id_tag_trader is not a valid string.
        :raises ValueError: If the id_tag_strategy is not a valid string.
        """
        self._clock = clock
        self._symbol_counts = {}  # type: Dict[Symbol, int]
        self.id_tag_trader = id_tag_trader
        self.id_tag_strategy = id_tag_strategy

    cdef str _generate(self, Symbol symbol):
        """
        Return a unique identifier string using the given symbol.

        :param symbol: The symbol for the unique identifier.
        :return: The unique identifier string.
        """
        if symbol not in self._symbol_counts:
            self._symbol_counts[symbol] = 0
        self._symbol_counts[symbol] += 1

        return (self._clock.get_datetime_tag()
                + SEPARATOR + self.id_tag_trader.value
                + SEPARATOR + self.id_tag_strategy.value
                + SEPARATOR + symbol.code
                + SEPARATOR + symbol.venue_string()
                + SEPARATOR + str(self._symbol_counts[symbol]))


cdef class OrderIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique OrderIds.
    """

    def __init__(self,
                 ValidString id_tag_trader,
                 ValidString id_tag_strategy,
                 Clock clock=LiveClock()):
        """
        Initializes a new instance of the OrderIdGenerator class.

        :param id_tag_trader: The identifier tag for the trader.
        :param id_tag_strategy: The identifier tag for the strategy.
        :param clock: The internal clock.
        """
        super().__init__(id_tag_trader,
                         id_tag_strategy,
                         clock)

    cpdef OrderId generate(self, Symbol symbol):
        """
        Return a unique OrderId using the given symbol.

        :param symbol: The symbol for the unique identifier.
        :return: The unique OrderId.
        """
        return OrderId(self._generate(symbol))


cdef class PositionIdGenerator(IdentifierGenerator):
    """
    Provides a generator for unique PositionIds.
    """

    def __init__(self,
                 ValidString id_tag_trader,
                 ValidString id_tag_strategy,
                 Clock clock=LiveClock()):
        """
        Initializes a new instance of the PositionIdGenerator class.

        :param id_tag_trader: The identifier tag for the trader.
        :param id_tag_strategy: The identifier tag for the strategy.
        :param clock: The internal clock.
        """
        super().__init__(id_tag_trader,
                         id_tag_strategy,
                         clock)

    cpdef PositionId generate(self, Symbol symbol):
        """
        Return a unique PositionId using the given symbol.

        :param symbol: The symbol for the unique identifier.
        :return: The unique PositionId.
        """
        return PositionId(self._generate(symbol))
