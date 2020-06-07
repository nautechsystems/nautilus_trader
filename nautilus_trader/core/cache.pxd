# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------


cdef class ObjectCache:
    cdef readonly type type_key
    cdef readonly type type_value

    cdef dict _cache
    cdef object _parser

    cpdef object get(self, str key)
    cpdef list keys(self)
    cpdef void clear(self) except *
