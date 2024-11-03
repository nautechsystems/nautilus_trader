# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

Functions include awareness/tz checks and conversions, as well as ISO 8601 conversion.
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
    dt : pd.Timestamp
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

cpdef str unix_nanos_to_str(uint64_t unix_nanos):
    """
    Convert the given `unix_nanos` to an ISO 8601 formatted string.

    Parameters
    ----------
    unix_nanos : int
        The UNIX timestamp (nanoseconds) to be converted.

    Returns
    -------
    str

    """
    return format_iso8601(unix_nanos_to_dt(unix_nanos))


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


cpdef str format_iso8601(datetime dt):
    """
    Format the given datetime to a millisecond accurate ISO 8601 specification string.

    Parameters
    ----------
    dt : datetime
        The datetime to format.

    Returns
    -------
    str

    """
    Condition.not_none(datetime, "datetime")

    # Note the below is faster than `.isoformat()` or string formatting by 25%
    # Have not tried char* manipulation
    cdef str tz_stripped = str(dt).replace(' ', 'T', 1).rpartition('+')[0]

    if not PyUnicode_Contains(tz_stripped, '.'):
        return f"{tz_stripped}.000Z"

    cdef tuple dt_partitioned = tz_stripped.rpartition('.')
    return f"{dt_partitioned[0]}.{dt_partitioned[2][:3]}Z"


def max_date_str(date1: str | int | None = None, date2: str | int | None = None) -> str | None:
    """
    Returns the maximum date as an ISO 8601 formatted string.

    Parameters
    ----------
    date1 : str | int | None, optional
        The first date to compare. Can be a string, integer (timestamp), or None. Default is None.
    date2 : str | int | None, optional
        The second date to compare. Can be a string, integer (timestamp), or None. Default is None.

    Returns
    -------
    str | None
        The maximum date as an ISO 8601 formatted string, or None if both input dates are None.

    """
    if date1 is None and date2 is None:
        return None

    if date1 is None:
        return pd.Timestamp(date2).isoformat()

    if date2 is None:
        return pd.Timestamp(date1).isoformat()

    return max(pd.Timestamp(date1), pd.Timestamp(date2)).isoformat()


def min_date_str(date1: str | int | None = None, date2: str | int | None = None) -> str | None:
    """
    Returns the minimum date as an ISO 8601 formatted string.

    Parameters
    ----------
    date1 : str | int | None, optional
        The first date to compare. Can be a string, integer (timestamp), or None. Default is None.
    date2 : str | int | None, optional
        The second date to compare. Can be a string, integer (timestamp), or None. Default is None.

    Returns
    -------
    str | None
        The minimum date as an ISO 8601 formatted string, or None if both input dates are None.

    """
    if date1 is None and date2 is None:
        return None

    if date1 is None:
        return pd.Timestamp(date2).isoformat()

    if date2 is None:
        return pd.Timestamp(date1).isoformat()

    return min(pd.Timestamp(date1), pd.Timestamp(date2)).isoformat()
