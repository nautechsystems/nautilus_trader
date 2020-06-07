# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import uuid

from nautilus_trader.core.correctness cimport Condition


cdef class ValidString:
    """
    Represents a valid string value. A valid string value cannot be None, empty or all white space.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the ValidString class.

        :param value: The value of the string.
        """
        Condition.valid_string(value, 'value')

        self.value = value

    cpdef str to_string(self, bint with_class=False):
        """
        Return the string representation of this object.
        
        :return: str.
        """
        if with_class:
            return f'{self.__class__.__name__}({self.value})'
        else:
            return self.value

    def __eq__(self, ValidString other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value == other.value

    def __ne__(self, ValidString other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value != other.value

    def __lt__(self, ValidString other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value.__lt__(other.value)

    def __le__(self, ValidString other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given
        object.

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
        Return a value indicating whether this object is greater than or equal to (>=) the given
        object.

        :param other: The other object.
        :return bool.
        """
        return self.value.__ge__(other.value)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.value)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return self.value

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self.__class__.__name__)}({str(self.value)}) object at {id(self)}>"


cdef class Label(ValidString):
    """
    Represents a label with a valid string value.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the Label class.

        :param value: The label identifier value.
        :raises ValueError: If the value is not a valid string.
        """
        super().__init__(value)


cdef class Identifier(ValidString):
    """
    The base class for all identifiers.
    """

    def __init__(self, str value):
        """
        Initializes a new instance of the Identifier class.

        :param value: The value of the identifier.
        """
        super().__init__(value)

        self.id_type = self.__class__.__name__

    cpdef bint equals(self, Identifier other):
        """
        Return a value indicating whether the given object is equal to this object.
        
        :param other: The other object to compare
        :return bool.
        """
        return self.id_type == other.id_type and self.value == other.value


cdef class GUID(Identifier):
    """
    Represents a globally unique identifier which is synonymous with UUID
    (universally unique identifier).
    """

    def __init__(self, value not None: uuid.UUID):
        """
        Initializes a new instance of the GUID class.

        :param value: The UUID value.
        :raises ValueError: If the value is not of type UUID.
        """
        Condition.type(value, uuid.UUID, 'value')
        super().__init__(str(value))

    @staticmethod
    cdef GUID none():
        """
        Return an empty GUID.

        Returns
        -------
            GUID
        """
        return GUID(uuid.UUID('00000000-0000-0000-0000-000000000000'))

    @staticmethod
    def py_none() -> GUID:
        """
        Return an empty GUID.

        Returns
        -------
            GUID
        """
        return GUID.none()
