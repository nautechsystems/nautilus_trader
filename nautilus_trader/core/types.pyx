# -------------------------------------------------------------------------------------------------
# <copyright file="types.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from uuid import UUID

from nautilus_trader.core.correctness cimport Condition


cdef class ValidString:
    """
    Represents a valid string value. A valid value cannot be None, empty or all white space.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the StringValue abstract class.

        :param value: The value of the string.
        """
        Condition.valid_string(value, 'value')

        self.value = value

    cpdef bint equals(self, ValidString other):
        """
        Return a value indicating whether the given object is equal to this object.
        
        :param other: The other object to compare
        :return bool.
        """
        if isinstance(other, self.__class__):
            return self.value == other.value
        else:
            return False

    cpdef str to_string(self):
        """
        Return a string representation of this object.
        
        :return: str.
        """
        return self.value

    def __eq__(self, ValidString other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, ValidString other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __lt__(self, ValidString other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value.__lt__(other.value)

    def __le__(self, ValidString other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value.__le__(other.value)

    def __gt__(self, ValidString other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value.__gt__(other.value)

    def __ge__(self, ValidString other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value.__ge__(other.value)

    def __hash__(self) -> int:
        """"
        Return a hash representation of this object.

        :return int.
        """
        return hash(self.value)

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return self.value

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self.__class__.__name__)}({str(self.value)}) object at {id(self)}>"


cdef class Identifier(ValidString):
    """
    The abstract base class for all identifiers.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the Identifier abstract class.

        :param value: The value of the identifier.
        """
        super().__init__(value)

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return f"{str(self.__class__.__name__)}({self.value})"


cdef class GUID(Identifier):
    """
    Represents a globally unique identifier.
    """

    def __init__(self, value: UUID):
        """
        Initializes a new instance of the GUID class.

        :param value: The value of the GUID (input must be of type UUID).
        :raises ConditionFailed: If the value is not of type UUID.
        """
        Condition.type(value, UUID, 'value')

        super().__init__(str(value))
