# -------------------------------------------------------------------------------------------------
# <copyright file="decimal.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

cdef class Decimal:
    cdef double _power
    cdef double _value

    cdef readonly int precision

    @staticmethod
    cdef Decimal zero()
    @staticmethod
    cdef Decimal from_string_to_decimal(str value)
    @staticmethod
    cdef int precision_from_string(str value) except -1
    cpdef bint equals(self, Decimal other)
    cpdef str to_string(self, bint format_commas=*)
    cpdef double as_double(self)
    cpdef object as_decimal(self)
    cpdef bint eq(self, Decimal other)
    cpdef bint ne(self, Decimal other)
    cpdef bint lt(self, Decimal other)
    cpdef bint le(self, Decimal other)
    cpdef bint gt(self, Decimal other)
    cpdef bint ge(self, Decimal other)
    cpdef Decimal add_decimal(self, Decimal other, bint keep_precision=*)
    cpdef Decimal subtract_decimal(self, Decimal other, bint keep_precision=*)
