# -------------------------------------------------------------------------------------------------
# <copyright file="types.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cdef class ValidString:
    cdef readonly str value
    cpdef bint equals(self, ValidString other)
    cpdef str to_string(self)


cdef class Identifier(ValidString):
    pass


cdef class GUID(Identifier):
    pass
