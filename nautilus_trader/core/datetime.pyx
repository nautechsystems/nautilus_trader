# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

"""
This module provides efficient functions for performing standard datetime related operations.

Functions include awareness/tz checks and conversions, as well as ISO 8601 (RFC 3339) conversion.
"""

import pandas as pd
import pytz
from pandas.api.types import is_datetime64_ns_dtype

# Re-exports
from nautilus_trader.core.nautilus_pyo3 import micros_to_nanos as micros_to_nanos
from nautilus_trader.core.nautilus_pyo3 import millis_to_nanos as millis_to_nanos
from nautilus_trader.core.nautilus_pyo3 import nanos_to_micros as nanos_to_micros
from nautilus_trader.core.nautilus_pyo3 import nanos_to_millis as nanos_to_millis
from nautilus_trader.core.nautilus_pyo3 import nanos_to_secs as nanos_to_secs
from nautilus_trader.core.nautilus_pyo3 import secs_to_millis as secs_to_millis
from nautilus_trader.core.nautilus_pyo3 import secs_to_nanos as secs_to_nanos

cimport cpython.datetime
from cpython.datetime cimport datetime_tzinfo
from cpython.unicode cimport PyUnicode_Contains
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport unix_nanos_to_iso8601_cstr
from nautilus_trader.core.rust.core cimport unix_nanos_to_iso8601_millis_cstr
from nautilus_trader.core.string cimport cstr_to_pystr


# UNIX epoch is the UTC time at 00:00:00 on 1/1/1970
# https://en.wikipedia.org/wiki/Unix_time
cdef datetime UNIX_EPOCH = pd.Timestamp("1970-01-01", tz=pytz.utc)


cpdef unix_nanos_to_dt(uint64_t nanos):
    """
    Return the datetime (UTC) from the given UNIX timestamp (nanoseconds).

    Parameters
    ----------
    nanos : uint64_t
        The UNIX timestamp (nanoseconds) to convert.

    Returns
    -------
    pd.Timestamp

    """
    return pd.Timestamp(nanos, unit="ns", tz=pytz.utc)


cpdef dt_to_unix_nanos(dt: pd.Timestamp):
    """
    Return the UNIX timestamp (nanoseconds) from the given datetime (UTC).

    Parameters
    ----------
    dt : pd.Timestamp | str | int
        The datetime to convert.

    Returns
    -------
    uint64_t

    Warnings
    --------
    This function expects a pandas `Timestamp` as standard Python `datetime`
    objects are only accurate to 1 microsecond (μs).

    """
    Condition.not_none(dt, "dt")

    if not isinstance(dt, pd.Timestamp):
        dt = pd.Timestamp(dt)

    return <uint64_t>dt.value


cpdef str unix_nanos_to_iso8601(uint64_t unix_nanos, bint nanos_precision = True):
    """
    Convert the given `unix_nanos` to an ISO 8601 (RFC 3339) format string.

    Parameters
    ----------
    unix_nanos : int
        The UNIX timestamp (nanoseconds) to be converted.
    nanos_precision : bool, default True
        If True, use nanosecond precision. If False, use millisecond precision.

    Returns
    -------
    str

    """
    if nanos_precision:
        return cstr_to_pystr(unix_nanos_to_iso8601_cstr(unix_nanos))
    else:
        return cstr_to_pystr(unix_nanos_to_iso8601_millis_cstr(unix_nanos))


cpdef str format_iso8601(datetime dt, bint nanos_precision = True):
    """
    Format the given datetime as an ISO 8601 (RFC 3339) specification string.

    Parameters
    ----------
    dt : pd.Timestamp
        The datetime to format.
    nanos_precision : bool, default True
        If True, use nanosecond precision. If False, use millisecond precision.

    Returns
    -------
    str

    """
    Condition.type(dt, pd.Timestamp, "dt")

    if nanos_precision:
        return cstr_to_pystr(unix_nanos_to_iso8601_cstr(dt.value))
    else:
        return cstr_to_pystr(unix_nanos_to_iso8601_millis_cstr(dt.value))


cpdef maybe_unix_nanos_to_dt(nanos):
    """
    Return the datetime (UTC) from the given UNIX timestamp (nanoseconds), or ``None``.

    If nanos is ``None``, then will return ``None``.

    Parameters
    ----------
    nanos : int, optional
        The UNIX timestamp (nanoseconds) to convert.

    Returns
    -------
    pd.Timestamp or ``None``

    """
    if nanos is None:
        return None
    else:
        return pd.Timestamp(nanos, unit="ns", tz=pytz.utc)


cpdef maybe_dt_to_unix_nanos(dt: pd.Timestamp):
    """
    Return the UNIX timestamp (nanoseconds) from the given datetime, or ``None``.

    If dt is ``None``, then will return ``None``.

    Parameters
    ----------
    dt : pd.Timestamp, optional
        The datetime to convert.

    Returns
    -------
    int64 or ``None``

    Warnings
    --------
    If the input is not ``None`` then this function expects a pandas `Timestamp`
    as standard Python `datetime` objects are only accurate to 1 microsecond (μs).

    """
    if dt is None:
        return None

    if not isinstance(dt, pd.Timestamp):
        dt = pd.Timestamp(dt)

    return <uint64_t>dt.value


cpdef bint is_datetime_utc(datetime dt):
    """
    Return a value indicating whether the given timestamp is timezone aware UTC.

    Parameters
    ----------
    dt : datetime
        The datetime to check.

    Returns
    -------
    bool
        True if timezone aware UTC, else False.

    """
    Condition.not_none(dt, "dt")

    return datetime_tzinfo(dt) == pytz.utc


cpdef bint is_tz_aware(time_object):
    """
    Return a value indicating whether the given object is timezone aware.

    Parameters
    ----------
    time_object : datetime, pd.Timestamp, pd.Series, pd.DataFrame
        The time object to check.

    Returns
    -------
    bool
        True if timezone aware, else False.

    """
    Condition.not_none(time_object, "time_object")

    if isinstance(time_object, datetime):
        return datetime_tzinfo(time_object) is not None
    elif isinstance(time_object, pd.DataFrame):
        return hasattr(time_object.index, "tz") or time_object.index.tz is not None
    else:
        raise ValueError(f"Cannot check timezone awareness of a {type(time_object)} object")


cpdef bint is_tz_naive(time_object):
    """
    Return a value indicating whether the given object is timezone naive.

    Parameters
    ----------
    time_object : datetime, pd.Timestamp, pd.DataFrame
        The time object to check.

    Returns
    -------
    bool
        True if object timezone naive, else False.

    """
    return not is_tz_aware(time_object)


cpdef datetime as_utc_timestamp(datetime dt):
    """
    Ensure the given timestamp is tz-aware UTC.

    Parameters
    ----------
    dt : datetime
        The timestamp to check.

    Returns
    -------
    datetime

    """
    Condition.not_none(datetime, "datetime")

    if dt.tzinfo is None:  # tz-naive
        return pytz.utc.localize(dt)
    elif dt.tzinfo != pytz.utc:
        return dt.astimezone(pytz.utc)
    else:
        return dt  # Already UTC


cpdef object as_utc_index(data: pd.DataFrame):
    """
    Ensure the given data has a DateTimeIndex which is tz-aware UTC.

    Parameters
    ----------
    data : pd.Series or pd.DataFrame.
        The object to ensure is UTC.

    Returns
    -------
    pd.Series, pd.DataFrame or ``None``

    """
    Condition.not_none(data, "data")

    if data.empty:
        return data

    # Ensure the index is localized to UTC
    if data.index.tzinfo is None:  # tz-naive
        data = data.tz_localize(pytz.utc)
    elif data.index.tzinfo != pytz.utc:
        data = data.tz_convert(None).tz_localize(pytz.utc)

    # Check if the index is in nanosecond resolution, convert if not
    if not is_datetime64_ns_dtype(data.index.dtype):
        data.index = data.index.astype("datetime64[ns, UTC]")

    return data


cpdef datetime time_object_to_dt(time_object):
    """
    Return the datetime (UTC) from the given UNIX timestamp as integer (nanoseconds), string or pd.Timestamp.

    Parameters
    ----------
    time_object : pd.Timestamp | str | int | None
        The time object to convert.

    Returns
    -------
    pd.Timestamp or ``None``
        Returns None if the input is None.

    """
    if time_object is None:
        return None

    if isinstance(time_object, pd.Timestamp):
        used_date = time_object
    else:
        used_date = pd.Timestamp(time_object)

    return as_utc_timestamp(used_date)



def max_date(date1: pd.Timestamp | str | int | None = None, date2: str | int | None = None) -> pd.Timestamp | None:
    """
    Return the maximum date as a datetime (UTC).

    Parameters
    ----------
    date1 : pd.Timestamp | str | int | None, optional
        The first date to compare. Can be a string, integer (timestamp), or None. Default is None.
    date2 : pd.Timestamp | str | int | None, optional
        The second date to compare. Can be a string, integer (timestamp), or None. Default is None.

    Returns
    -------
    pd.Timestamp | None
        The maximum date, or None if both input dates are None.

    """
    if date1 is None and date2 is None:
        return None

    if date1 is None:
        return time_object_to_dt(date2)

    if date2 is None:
        return time_object_to_dt(date1)

    return max(time_object_to_dt(date1), time_object_to_dt(date2))


def min_date(date1: pd.Timestamp | str | int | None = None, date2: str | int | None = None) -> pd.Timestamp | None:
    """
    Return the minimum date as a datetime (UTC).

    Parameters
    ----------
    date1 : pd.Timestamp | str | int | None, optional
        The first date to compare. Can be a string, integer (timestamp), or None. Default is None.
    date2 : pd.Timestamp | str | int | None, optional
        The second date to compare. Can be a string, integer (timestamp), or None. Default is None.

    Returns
    -------
    pd.Timestamp | None
        The minimum date, or None if both input dates are None.

    """
    if date1 is None and date2 is None:
        return None

    if date1 is None:
        return time_object_to_dt(date2)

    if date2 is None:
        return time_object_to_dt(date1)

    return min(time_object_to_dt(date1), time_object_to_dt(date2))
