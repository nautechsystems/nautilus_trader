# -------------------------------------------------------------------------------------------------
# <copyright file="functions.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime


cpdef double fast_round(double value, int precision)
cpdef double fast_mean(iterable)
cpdef double basis_points_as_percentage(double basis_points)
cdef long get_size_of(obj)
cpdef str format_size(double size, int precision=*)
cpdef str format_bytes(double size)
cpdef str pad_string(str string, int length, str pad=*)
cpdef str format_zulu_datetime(datetime dt, bint with_t=*)
cpdef object with_utc_index(dataframe)
cpdef object as_utc_timestamp(datetime timestamp)
