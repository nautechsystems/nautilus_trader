# -------------------------------------------------------------------------------------------------
# <copyright file="types.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from uuid import UUID

from nautilus_trader.core.precondition cimport Precondition


cdef class ValidString:
    """
    Represents a previously validated string (validated with Precondition.valid_string()).
    """

    def __init__(self, str value=None):
        """
        Initializes a new instance of the ValidString class.

        :param value: The string value to validate.
        """
        if value is None or value == '':
            value = 'NONE'
        else:
            Precondition.valid_string(value, 'value')

        self.value = value

    @staticmethod
    cdef ValidString none():
        """
        Return a valid string with a value of 'NONE'.
        
        :return: ValidString.
        """
        return ValidString()

    cdef bint equals(self, ValidString other):
        """
        Compare if the object equals the given object.
        
        :param other: The other string to compare
        :return: True if the objects are equal, otherwise False.
        """
        return self.value == other.value

    def __eq__(self, ValidString other) -> bool:
        """
        Override the default equality comparison.
        """
        return self.equals(other)

    def __ne__(self, ValidString other) -> bool:
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
        :return: The str() string representation of the valid string.
        """
        return self.value

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the valid string.
        """
        return f"<{self.__class__.__name__}({self.value}) object at {id(self)}>"


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

        :param value: The value of the GUID (input must be of type UUID).
        :raises ValueError: If the value is not of type UUID.
        """
        Precondition.type(value, UUID, 'value')

        super().__init__(str(value))
