# -------------------------------------------------------------------------------------------------
# <copyright file="functions.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd
import pytz

from libc.math cimport round
from cpython.datetime cimport datetime
from cpython.unicode cimport PyUnicode_Contains


cpdef double fast_round(double value, int precision):
    """
    Return the given value rounded to the nearest precision digits.
    
    :param value: The value to round.
    :param precision: The precision to round to.
    :return: double.
    """
    cdef int power = 10 ** precision
    return round(value * power) / power


cpdef double basis_points_as_percentage(double basis_points):
    """
    Return the given basis points expressed as a percentage where 100% = 1.0.
    
    :param basis_points: The basis points to convert to percentage.
    :return double.
    """
    return basis_points * 0.0001


cpdef str pad_string(str string, int length, str pad=' '):
    """
    Return the given string front padded.

    :param string: The string to pad.
    :param length: The length to pad to.
    :param pad: The padding character.
    :return str.
    """
    return ((length - len(string)) * pad) + string


cpdef str format_zulu_datetime(datetime dt):
    """
    Return the formatted string from the given datetime.
    
    :param dt: The datetime to format.
    :return str.
    """
    cdef str formatted_dt = ''
    cdef tuple dt_partitioned
    cdef str end

    try:
        formatted_dt = dt.isoformat(timespec='microseconds').partition('+')[0][:-3]
    except TypeError as ex:
        formatted_dt = dt.isoformat().partition('+')[0]
    if not PyUnicode_Contains(formatted_dt, '.'):
        return formatted_dt + '.000Z'
    else:
        dt_partitioned = formatted_dt.rpartition('.')
        end = dt_partitioned[2]
        if len(end) > 3:
            end = end[:3]
        return f'{dt_partitioned[0]}.{end}Z'


cpdef object with_utc_index(dataframe):
        """
        Return the given pandas DataFrame with the index timestamps localized 
        or converted to UTC. If the DataFrame is None then returns None.
        
        :param dataframe: The pd.DataFrame to localize.
        :return pd.DataFrame or None.
        """
        if dataframe is not None:
            if not hasattr(dataframe.index, 'tz') or dataframe.index.tz is None:  # tz-naive
                return dataframe.tz_localize('UTC')
            elif dataframe.index.tz != pytz.UTC:
                return dataframe.tz_convert('UTC')
            else:
                return dataframe  # Already UTC
        return dataframe  # The input argument was None


cpdef object as_utc_timestamp(datetime timestamp):
    """
    Return the given timestamp converted to a pandas timestamp and UTC as required.
    
    :param timestamp: The timestamp to convert.
    :return pd.Timestamp.
    """
    if not isinstance(timestamp, pd.Timestamp):
        timestamp = pd.Timestamp(timestamp)

    if timestamp.tz is None:  # tz-naive
        return timestamp.tz_localize('UTC')
    elif timestamp.tz != pytz.UTC:
        return timestamp.tz_convert('UTC')
    else:
        return timestamp  # Already UTC


# Closures in cpdef functions not yet supported (21/6/19)
def max_in_dict(dict dictionary):
    """
    Return the key for the maximum value held in the given dictionary.
    
    :param dictionary: The dictionary to check.
    :return The key.
    """
    return max(dictionary.items(), key=lambda x: x[1])[0]
