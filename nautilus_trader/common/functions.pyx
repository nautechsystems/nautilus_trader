# -------------------------------------------------------------------------------------------------
# <copyright file="functions.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import gc
import sys
import pandas as pd
import pytz
from libc.math cimport round
from cpython.datetime cimport datetime
from cpython.unicode cimport PyUnicode_Contains

from nautilus_trader.core.correctness cimport Condition


cpdef double fast_round(double value, int precision):
    """
    Return the given value rounded to the nearest precision digits.
    
    :param value: The value to round.
    :param precision: The precision to round to.
    :return: double.
    """
    Condition.not_negative_int(precision, 'precision')

    cdef int power = 10 ** precision
    return round(value * power) / power


cpdef double fast_mean(iterable):
    """
    Return the average value of the iterable.
    
    :param iterable: The iterable to evaluate.
    :return: double.
    """
    Condition.not_none(iterable, 'iterable')

    cdef int length = len(iterable)

    if length == 0:
        return 0.0

    cdef double total = 0.0
    for i in range(length):
        total += iterable[i]
    return total / length


cpdef double basis_points_as_percentage(double basis_points):
    """
    Return the given basis points expressed as a percentage where 100% = 1.0.
    
    :param basis_points: The basis points to convert to percentage.
    :return double.
    """
    return basis_points * 0.0001


cdef long get_size_of(obj):
    Condition.not_none(obj, 'obj')

    cdef set marked = {id(obj)}
    obj_q = [obj]
    cdef long size = 0

    while obj_q:
        size += sum(map(sys.getsizeof, obj_q))

        # Lookup all the object referred to by the object in obj_q.
        # See: https://docs.python.org/3.7/library/gc.html#gc.get_referents
        all_refs = ((id(o), o) for o in gc.get_referents(*obj_q))

        # Filter object that are already marked.
        # Using dict notation will prevent repeated objects.
        new_ref = {
            o_id: o for o_id, o in all_refs if o_id not in marked and not isinstance(o, type)
        }

        # The new obj_q will be the ones that were not marked,
        # and we will update marked with their ids so we will
        # not traverse them again.
        obj_q = new_ref.values()
        marked.update(new_ref.keys())

    return size


cdef dict POWER_LABELS = {
    0: 'bytes',
    1: 'KB',
    2: 'MB',
    3: 'GB',
    4: 'TB'
}

cpdef str format_bytes(double size):
    """
    Return the formatted bytes size.

    :param size: The size in bytes.
    :return: str.
    """
    Condition.not_negative(size, 'size')

    cdef double power = pow(2, 10)

    cdef int n = 0
    while size >= power:
        size /= power
        n += 1
    return f'{fast_round(size, 2):,} {POWER_LABELS[n]}'


cpdef str pad_string(str string, int length, str pad=' '):
    """
    Return the given string front padded.

    :param string: The string to pad.
    :param length: The length to pad to.
    :param pad: The padding character.
    :return str.
    """
    Condition.not_none(string, 'string')
    Condition.not_negative_int(length, 'length')
    Condition.not_none(pad, 'pad')

    return ((length - len(string)) * pad) + string


cpdef str format_iso8601(datetime dt, bint with_t=True):
    """
    Return the ISO 8601 formatted string for the given datetime.
    
    :param dt: The input datetime to format.
    :param with_t: If the datetime should be formatted with 'T' separating time.
    :return str.
    """
    Condition.not_none(dt, 'dt')

    cdef str tz_stripped = str(dt).rpartition('+')[0]

    if with_t:
        tz_stripped = tz_stripped.replace(' ', 'T')

    if not PyUnicode_Contains(tz_stripped, '.'):
        return tz_stripped + '.000Z'

    cdef tuple dt_partitioned = tz_stripped.rpartition('.')
    return f'{dt_partitioned[0]}.{dt_partitioned[2][:3]}Z'


cpdef object with_utc_index(dataframe: pd.DataFrame):
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


cpdef datetime as_utc_timestamp(datetime timestamp):
    """
    Return the given timestamp converted to a pandas timestamp and UTC as required.
    
    :param timestamp: The timestamp to convert.
    :return pd.Timestamp.
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


# Closures in cpdef functions not yet supported (21/6/19)
def max_in_dict(dict dictionary):
    """
    Return the key for the maximum value held in the given dictionary.

    :param dictionary: The dictionary to check.
    :return The key.
    """
    Condition.not_none(dictionary, 'dictionary')

    return max(dictionary.items(), key=lambda x: x[1])[0]


# Function only exists due to some limitation with Cython and closures created by the slice
def slice_dataframe(dataframe, start, end) -> pd.DataFrame:
    """
    Return the dataframe sliced using the given arguments.

    :param dataframe: The dataframe to slice.
    :param start: The start of the slice.
    :param end: The end of the slice.
    :return: pd.DataFrame.
    """
    if dataframe is None:
        return pd.DataFrame()

    return dataframe[start:end]
