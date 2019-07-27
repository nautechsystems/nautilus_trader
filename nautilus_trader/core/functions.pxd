# -------------------------------------------------------------------------------------------------
# <copyright file="functions.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime


cpdef str pad_string(str string, int length)
cpdef str format_zulu_datetime(datetime dt)
cpdef float basis_points_as_percentage(float basis_points)
cpdef object with_utc_index(dataframe)
cpdef object as_utc_timestamp(datetime timestamp)
