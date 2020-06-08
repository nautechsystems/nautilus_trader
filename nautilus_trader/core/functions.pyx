# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

# cython: boundscheck=False
# cython: wraparound=False

import gc
import sys
import pandas as pd
from libc.math cimport round

from nautilus_trader.core.correctness cimport Condition


cpdef double fast_round(double value, int precision):
    """
    Return the given value rounded to the nearest precision digits.
    
    :param value: The value to round.
    :param precision: The precision to round to.
    :return: double.
    """
    cdef int power = 10 ** precision
    return round(value * power) / power


cpdef double fast_mean(list values):
    """
    Return the average value of the iterable.
    
    :param values: The iterable to evaluate.
    :return: double.
    """
    cdef int length = len(values)

    if length == 0:
        return 0.0

    cdef double total = 0.0
    cdef int i
    for i in range(length):
        total += values[i]
    return total / length


cpdef double fast_mean_iterated(
        list values,
        double next_value,
        double current_value,
        int expected_length,
        bint drop_left=True):
    """
    Return the calculated average from the given inputs.
    
    :param values: The values for the calculation.
    :param next_value: The next input value for the average.
    :param current_value: The current value for the average.
    :param expected_length: The expected length of the inputs.
    :param drop_left: If the value to be dropped should be from the left side
    of the inputs (index 0).
    :return: double.
    """
    cdef int length = len(values)
    if length < expected_length:
        return fast_mean(values)

    assert length == expected_length

    cdef double value_to_drop = values[0] if drop_left else values[length - 1]
    return current_value + ((next_value - value_to_drop) / length)


cpdef double basis_points_as_percentage(double basis_points):
    """
    Return the given basis points expressed as a percentage where 100% = 1.0.
    
    :param basis_points: The basis points to convert to percentage.
    :return double.
    """
    return basis_points * 0.0001


# Closures in cpdef functions not yet supported (10/02/20)
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
