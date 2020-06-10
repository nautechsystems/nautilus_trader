# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
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

import pandas as pd
import pytz
from cpython.datetime cimport datetime
from cpython.unicode cimport PyUnicode_Contains

from nautilus_trader.core.correctness cimport Condition


cpdef bint is_tz_aware(time_object):
    """
    Checks if the given object is timezone aware. The object must be either
    datetime, pd.Timestamp or pd.DataFrame
    
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
    elif isinstance(time_object, (pd.Series, pd.DataFrame)):
        return hasattr(time_object.index, 'tz') or time_object.index.tz is not None
    else:
        raise ValueError(f"Cannot check timezone awareness of a {type(time_object)} object.")


cpdef bint is_tz_naive(time_object):
    """
    Checks if the given object is timezone naive. The object must be either
    datetime, pd.Timestamp or pd.DataFrame
    
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


cpdef datetime as_timestamp_utc(datetime timestamp):
    """
    Return the given timestamp converted to a UTC timezone aware pd.Timestamp.

    Parameters
    ----------
    timestamp : datetime
        The timestamp to convert.
    
    Returns
    -------
    pd.Timestamp

    """
    Condition.not_none(timestamp, 'timestamp')

    if not isinstance(timestamp, pd.Timestamp):
        timestamp = pd.Timestamp(timestamp)

    if timestamp.tz is None:  # tz-naive
        return timestamp.tz_localize('UTC')
    elif timestamp.tz != pytz.UTC:
        return timestamp.tz_convert('UTC')
    else:
        return timestamp  # Already UTC


cpdef object with_utc_index(dataframe: pd.DataFrame):
    """
    Return the given pandas DataFrame with the index timestamps localized
    or converted to UTC. If the DataFrame is None then returns None.
    
    Parameters
    ----------
    dataframe : pd.DataFrame.
        The object with DatetimeIndex to localize.
        
    Returns
    -------
    pd.DataFrame or None.

    """
    if dataframe is None:
        return dataframe

    if not hasattr(dataframe.index, 'tz') or dataframe.index.tz is None:  # tz-naive
        return dataframe.tz_localize('UTC')
    elif dataframe.index.tz != pytz.UTC:
        return dataframe.tz_convert('UTC')
    else:
        return dataframe  # Already UTC


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
    Condition.not_none(dt, 'dt')

    cdef str tz_stripped = str(dt).replace(' ', 'T').rpartition('+')[0]

    if not PyUnicode_Contains(tz_stripped, '.'):
        return tz_stripped + '.000Z'

    cdef tuple dt_partitioned = tz_stripped.rpartition('.')
    return f'{dt_partitioned[0]}.{dt_partitioned[2][:3]}Z'
