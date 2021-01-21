# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
This module provides efficient functions for performing standard datetime
related operations. Functions include awareness/tz checks and conversions, as
well as ISO 8601 conversion.
"""

import pandas as pd
import pytz

from cpython.datetime cimport datetime
from cpython.datetime cimport datetime_tzinfo
from cpython.datetime cimport timedelta
from cpython.unicode cimport PyUnicode_Contains

from nautilus_trader.core.correctness cimport Condition

# Unix epoch is the UTC time at 00:00:00 on 1/1/1970
UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=pytz.utc)


cpdef long to_posix_ms(datetime timestamp) except *:
    """
    Returns the POSIX millisecond timestamp from the given object.

    Parameters
    ----------
    timestamp : datetime
        The datetime for the timestamp.

    Returns
    -------
    int

    """
    return <long>((timestamp - UNIX_EPOCH).total_seconds() * 1000)


cpdef datetime from_posix_ms(long posix):
    """
    Returns the datetime in UTC from the given POSIX millisecond timestamp.

    Parameters
    ----------
    posix : int
        The timestamp to convert.

    Returns
    -------
    datetime

    """
    return UNIX_EPOCH + timedelta(milliseconds=posix)  # Round off thousands


cpdef bint is_datetime_utc(datetime timestamp) except *:
    """
    Return a value indicating whether the given timestamp is timezone aware UTC.

    Parameters
    ----------
    timestamp : datetime
        The datetime to check.

    Returns
    -------
    bool
        True if timezone aware UTC, else False.

    """
    Condition.not_none(timestamp, "timestamp")

    return datetime_tzinfo(timestamp) == pytz.utc


cpdef bint is_tz_aware(time_object) except *:
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


cpdef bint is_tz_naive(time_object) except *:
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


cpdef datetime as_utc_timestamp(datetime timestamp):
    """
    Ensure the given timestamp is a tz-aware UTC pd.Timestamp.

    Parameters
    ----------
    timestamp : datetime
        The timestamp to ensure is UTC.

    Returns
    -------
    pd.Timestamp

    """
    Condition.not_none(datetime, "datetime")

    if not isinstance(timestamp, pd.Timestamp):
        timestamp = pd.Timestamp(timestamp)

    if timestamp.tz is None:  # tz-naive
        return timestamp.tz_localize(pytz.utc)
    elif timestamp.tz != pytz.utc:
        return timestamp.tz_convert(pytz.utc)
    else:
        return timestamp  # Already UTC


cpdef object as_utc_index(data: pd.DataFrame):
    """
    Ensure the given data has a DateTimeIndex which is tz-aware UTC.

    Parameters
    ----------
    data : pd.Series or pd.DataFrame.
        The object to ensure is UTC.

    Returns
    -------
    pd.Series, pd.DataFrame or None

    """
    Condition.not_none(data, "data")

    if data.empty:
        return data

    if not hasattr(data.index, "tz") or data.index.tz is None:  # tz-naive
        return data.tz_localize(pytz.utc)
    elif data.index.tz != pytz.utc:
        return data.tz_convert(pytz.utc)
    else:
        return data  # Already UTC


cpdef str format_iso8601(datetime dt):
    """
    Format the given string to the ISO 8601 specification with "Z" zulu.

    Parameters
    ----------
    dt : datetime
        The input datetime to format.

    Notes
    -----
    Unit accuracy is millisecond.

    Returns
    -------
    str
        The formatted string.

    """
    Condition.not_none(datetime, "datetime")

    # Note the below is faster than .isoformat() or string formatting by 25%
    # Have not tried char* manipulation
    cdef str tz_stripped = str(dt).replace(' ', 'T', 1).rpartition('+')[0]

    if not PyUnicode_Contains(tz_stripped, '.'):
        return f"{tz_stripped}.000Z"

    cdef tuple dt_partitioned = tz_stripped.rpartition('.')
    return f"{dt_partitioned[0]}.{dt_partitioned[2][:3]}Z"
