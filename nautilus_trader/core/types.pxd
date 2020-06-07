# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------


cdef class ValidString:
    cdef readonly str value

    cpdef str to_string(self, bint with_class=*)


cdef class Label(ValidString):
    pass


cdef class Identifier(ValidString):
    cdef readonly str id_type

    cpdef bint equals(self, Identifier other)


cdef class GUID(Identifier):
    @staticmethod
    cdef GUID none()
