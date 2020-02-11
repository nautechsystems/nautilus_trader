# -------------------------------------------------------------------------------------------------
# <copyright file="functions.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime


cpdef double fast_round(double value, int precision)
cpdef double fast_mean(list values)
cpdef double fast_mean_iterated(
        list values,
        double next_value,
        double current_value,
        int expected_length,
        bint drop_left=*)
cpdef double basis_points_as_percentage(double basis_points)
cdef long get_size_of(obj)
cpdef str format_bytes(double size)
cpdef str pad_string(str string, int length, str pad=*)
cpdef str format_iso8601(datetime dt)
cpdef object with_utc_index(dataframe)
cpdef datetime as_timestamp_utc(datetime timestamp)
