# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

# cython: boundscheck=False
# cython: wraparound=False

import pytz
import pandas as pd
from cpython.datetime cimport datetime
from cpython.unicode cimport PyUnicode_Contains

from nautilus_trader.core.correctness cimport Condition


cpdef bint is_datetime_utc(datetime timestamp):
    """
    Checks if the given timestamp is timezone aware UTC.
    Will also return False if timezone is timezone.utc to standardize on pytz.
    
    Parameters
    ----------
    timestamp : datetime

    Returns
    -------
    bool
        True if argument timezone aware UTC, else False.
        
    """
    Condition.not_none(timestamp, 'timestamp')

    return timestamp.tzinfo == pytz.utc


cpdef bint is_tz_aware(time_object):
    """
    Checks if the given object is timezone aware.

    Parameters
    ----------
    time_object : datetime, pd.Timestamp, pd.Series, pd.DataFrame
        The time object to check.

    Returns
    -------
    bool
        True if object timezone aware, else False.

    """
    if isinstance(time_object, datetime):
        return time_object.tzinfo is not None
    elif isinstance(time_object, pd.Timestamp):
        return time_object.tz is not None
    elif isinstance(time_object, pd.DataFrame):
        return hasattr(time_object.index, 'tz') or time_object.index.tz is not None
    else:
        raise ValueError(f"Cannot check timezone awareness of a {type(time_object)} object.")


cpdef bint is_tz_naive(time_object):
    """
    Checks if the given object is timezone naive.
    
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
    Condition.not_none(datetime, 'datetime')

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
    pd.Series or pd.DataFrame or None

    """
    if data is None:
        return data

    if not hasattr(data.index, 'tz') or data.index.tz is None:  # tz-naive
        return data.tz_localize(pytz.utc)
    elif data.index.tz != pytz.utc:
        return data.tz_convert(pytz.utc)
    else:
        return data  # Already UTC


cpdef str format_iso8601(datetime dt):
    """
    Format the given string to the ISO 8601 specification with 'Z' zulu.
    
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
    Condition.not_none(datetime, 'datetime')

    cdef str tz_stripped = str(dt).replace(' ', 'T').rpartition('+')[0]

    if not PyUnicode_Contains(tz_stripped, '.'):
        return f'{tz_stripped}.000Z'

    cdef tuple dt_partitioned = tz_stripped.rpartition('.')
    return f'{dt_partitioned[0]}.{dt_partitioned[2][:3]}Z'
