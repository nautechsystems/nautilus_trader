# -------------------------------------------------------------------------------------------------
# <copyright file="types.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

cdef class StringValue:
    cdef readonly str value
    cpdef bint equals(self, StringValue other)


cdef class ValidString(StringValue):
    pass


cdef class Identifier(StringValue):
    pass


cdef class GUID(Identifier):
    pass
