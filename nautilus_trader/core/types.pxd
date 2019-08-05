# -------------------------------------------------------------------------------------------------
# <copyright file="types.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

cdef class ValidString:
    """
    Represents a validated string (validated with Condition.valid_string()).
    """
    cdef readonly str value
    @staticmethod
    cdef ValidString none()
    cdef bint equals(self, ValidString other)


cdef class Identifier:
    """
    Represents an identifier.
    """
    cdef readonly str value
    cpdef bint equals(self, Identifier other)


cdef class GUID(Identifier):
    """
    Represents a globally unique identifier.
    """
