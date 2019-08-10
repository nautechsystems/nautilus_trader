# -------------------------------------------------------------------------------------------------
# <copyright file="types.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from uuid import UUID

from nautilus_trader.core.correctness cimport Condition


cdef class StringValue:
    """
    The abstract base class for all string values.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the StringValue abstract class.

        :param value: The value of the string.
        """
        Condition.valid_string(value, 'value')

        self.value = value

    cpdef bint equals(self, StringValue other):
        """
        Return a value indicating whether the given object is equal to this object.
        
        :param other: The other object to compare
        :return: True if the objects are equal, otherwise False.
        """
        if isinstance(other, self.__class__):
            return self.value == other.value
        else:
            return False

    def __eq__(self, StringValue other) -> bool:
        """
        Return a value indicating whether the given object is equal to this object.

        :param other: The other object.
        :return: True if the objects are equal, otherwise False.
        """
        return self.equals(other)

    def __ne__(self, StringValue other) -> bool:
        """
        Return a value indicating whether the object is not equal to this object.

        :param other: The other object.
        :return: True if the objects are not equal, otherwise False.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash for this object.

        :return: int.
        """
        return hash(self.value)

    def __str__(self) -> str:
        """
        Return the str() representation of the object.

        :return: str.
        """
        return self.value

    def __repr__(self) -> str:
        """
        Return the repr() representation of the object.

        :return: str.
        """
        return f"<{str(self.__class__.__name__)}({str(self.value)}) object at {id(self)}>"


cdef class ValidString(StringValue):
    """
    Represents a previously validated string (validated with Condition.valid_string()).
    """

    def __init__(self, str value=None):
        """
        Initializes a new instance of the ValidString class.

        :param value: The string value to validate.
        """
        if value is None or value == '':
            value = 'NONE'

        super().__init__(value)


cdef class Identifier(StringValue):
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
        Return the str() representation of the object.

        :return: str.
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
        :raises ValueError: If the value is not of type UUID.
        """
        Condition.type(value, UUID, 'value')

        super().__init__(str(value))
