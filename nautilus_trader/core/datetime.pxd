import pandas as pd

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t


cpdef unix_nanos_to_dt(uint64_t nanos)
cpdef dt_to_unix_nanos(dt: pd.Timestamp)
cpdef str unix_nanos_to_iso8601(uint64_t unix_nanos, bint nanos_precision=*)
cpdef str format_iso8601(datetime dt, bint nanos_precision=*)
cpdef str format_optional_iso8601(datetime dt, bint nanos_precision=*)
cpdef maybe_unix_nanos_to_dt(nanos)
cpdef maybe_dt_to_unix_nanos(dt: pd.Timestamp)
cpdef bint is_datetime_utc(datetime dt)
cpdef bint is_tz_aware(time_object)
cpdef bint is_tz_naive(time_object)
cpdef datetime as_utc_timestamp(datetime dt)
cpdef object as_utc_index(time_object)
cpdef datetime time_object_to_dt(time_object)
