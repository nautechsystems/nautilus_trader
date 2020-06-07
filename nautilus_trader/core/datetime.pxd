# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime


cpdef bint is_tz_aware(dataframe)
cpdef bint is_tz_naive(dataframe)
cpdef str format_iso8601(datetime dt)
cpdef object with_utc_index(dataframe)
cpdef datetime as_timestamp_utc(datetime timestamp)
