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

import gc
import sys

import cython

cimport numpy as np
from libc.math cimport llround as llround_func
from libc.math cimport lround as lround_func
from libc.math cimport pow
from libc.math cimport sqrt
from libc.stdint cimport uint8_t

from nautilus_trader.core.correctness cimport Condition


# Determine correct C lround function
cdef round_func_type _get_round_func():
    if sizeof(long) == 8:
        return <round_func_type>lround_func
    elif sizeof(long long) == 8:
        return <round_func_type>llround_func
    else:
        raise TypeError(f"Can't support 'C' lround function.")

lround = _get_round_func()


@cython.boundscheck(False)
@cython.wraparound(False)
cdef inline uint8_t precision_from_str(str value):
    """
    Return the decimal precision inferred from the number of digits after
    the '.' decimal place.

    Parameters
    ----------
    value : str
        The string value to parse.

    Returns
    -------
    uint8

    Raises
    ------
    ValueError
        If value is not a valid string.

    Notes
    -----
    If no decimal place then precision will be inferred as zero.

    """
    Condition.valid_string(value, "value")

    return len(value.partition('.')[2])  # If does not contain "." then partition[2] will be ""


@cython.boundscheck(False)
@cython.wraparound(False)
cpdef inline int bisect_double_left(list a, double x, int lo=0, hi=None) except *:
    """
    Return the index where to insert item x in list a, assuming a is sorted.
    The return value i is such that all e in a[:i] have e <= x, and all e in
    a[i:] have e > x.  So if x already appears in the list, a.insert(i, x) will
    insert just after the rightmost x already there.
    Optional args lo (default 0) and hi (default len(a)) bound the
    slice of a to be searched.

    Returns
    -------
    int

    Raises
    ------
    ValueError
        If lo is negative (< 0).

    """
    Condition.not_negative_int(lo, "lo")

    if hi is None:
        hi = len(a)
    # Note, the comparison uses "<" to match the
    # __lt__() logic in list.sort() and in heapq.
    cdef int mid
    while lo < hi:
        mid = (lo + hi) // 2
        if a[mid] < x:
            lo = mid + 1
        else:
            hi = mid
    return lo


@cython.boundscheck(False)
@cython.wraparound(False)
cpdef inline int bisect_double_right(list a, double x, int lo=0, hi=None) except *:
    """
    Return the index where to insert item x in list a, assuming a is sorted.
    The return value i is such that all e in a[:i] have e <= x, and all e in
    a[i:] have e > x.  So if x already appears in the list, a.insert(i, x) will
    insert just after the rightmost x already there.
    Optional args lo (default 0) and hi (default len(a)) bound the
    slice of a to be searched.

    Returns
    -------
    int

    Raises
    ------
    ValueError
        If lo is negative (< 0).

    """
    Condition.not_negative_int(lo, "lo")

    if hi is None:
        hi = len(a)
    # Note, the comparison uses "<" to match the
    # __lt__() logic in list.sort() and in heapq.
    cdef int mid
    while lo < hi:
        mid = (lo + hi) // 2
        if x < a[mid]:
            hi = mid
        else:
            lo = mid + 1
    return lo


@cython.boundscheck(False)
@cython.wraparound(False)
cpdef double fast_mean(np.ndarray values) except *:
    """
    Return the average value for numpy.ndarray values

    Parameters
    ----------
    values : numpy.ndarray
        The array to evaluate.

    Returns
    -------
    double

    Notes
    -----
    > 10x faster than `np.mean` if the array length < ~200.

    """
    if values is None or values.ndim != 1:
        raise ValueError(f"values must be valid numpy.ndarray with ndim == 1.")

    cdef double[:] mv = values
    cdef int length = len(mv)

    if length == 0:
        return 0.0

    cdef double total = 0.0
    cdef int i
    with nogil:
        for i in range(length):
            total += mv[i]

    return total / length


@cython.boundscheck(False)
@cython.wraparound(False)
cpdef inline double fast_mean_iterated(
    np.ndarray values,
    double next_value,
    double current_value,
    int expected_length,
    bint drop_left=True,
) except *:
    """
    Return the calculated average from the given inputs.

    Parameters
    ----------
    values : list[double]
        The values for the calculation.
    next_value : double
        The next input value for the average.
    current_value : double
        The current value for the average.
    expected_length : int
        The expected length of the inputs.
    drop_left : bool
        If the value to be dropped should be from the left side of the inputs
        (index 0).

    Returns
    -------
    double

    Notes
    -----
    > 10x faster than `np.mean`.

    """
    if values is None or values.ndim != 1:
        raise ValueError(f"values must be valid ndarray with ndim == 1.")

    cdef double[:] mv = values
    cdef int length = len(mv)

    if length < expected_length:
        return fast_mean(values)

    assert length == expected_length

    cdef double value_to_drop = mv[0] if drop_left else mv[length - 1]
    return current_value + (next_value - value_to_drop) / length


cpdef inline double fast_std(np.ndarray values) except *:
    """
    Return the standard deviation from the given values.

    Parameters
    ----------
    values : numpy.ndarray
        The array for the calculation.

    Returns
    -------
    double

    Notes
    -----
    > 10x faster than `np.std`.

    """
    return fast_std_with_mean(values, fast_mean(values))


@cython.boundscheck(False)
@cython.wraparound(False)
cpdef double fast_std_with_mean(np.ndarray values, double mean) except *:
    """
    Return the standard deviation from the given values and mean.

    Parameters
    ----------
    values : numpy.ndarray
        The array for the calculation.
    mean : double
        The pre-calculated mean of the given values.

    Returns
    -------
    double

    Notes
    -----
    > 25x faster than `np.std` if the array length < ~200.

    """
    if values is None or values.ndim != 1:
        raise ValueError(f"values must be valid ndarray with ndim == 1.")

    cdef double[:] mv = values
    cdef int length = len(mv)

    if length == 0:
        return 0.0

    cdef double std_dev = 0.0
    cdef double v
    cdef int i
    with nogil:
        for i in range(length):
            v = mv[i] - mean
            std_dev += v * v

    return sqrt(std_dev / length)


cpdef inline double basis_points_as_percentage(double basis_points) except *:
    """
    Return the given basis points expressed as a percentage where 100% = 1.0.

    Parameters
    ----------
    basis_points : double
        The basis points to convert to percentage.

    Returns
    -------
    double

    Notes
    -----
    1 basis point = 0.01%.

    """
    return basis_points * 0.0001


def get_size_of(obj):
    """
    Return the bytes size in memory of the given object.

    Parameters
    ----------
    obj : object
        The object to analyze.

    Returns
    -------
    uint64

    """
    Condition.not_none(obj, "obj")

    cdef set marked = {id(obj)}
    obj_q = [obj]
    size = 0

    while obj_q:
        size += sum(map(sys.getsizeof, obj_q))

        # Lookup all the object referred to by the object in obj_q.
        # See: https://docs.python.org/3.7/library/gc.html#gc.get_referents
        all_refs = [(id(o), o) for o in gc.get_referents(*obj_q)]

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
    0: "bytes",
    1: "KB",
    2: "MB",
    3: "GB",
    4: "TB"
}

cpdef inline str format_bytes(double size):
    """
    Return the formatted bytes size.

    Parameters
    ----------
    size : double
        The size in bytes.

    Returns
    -------
    str

    """
    Condition.not_negative(size, "size")

    cdef double power = pow(2, 10)

    cdef int n = 0
    while size >= power:
        size /= power
        n += 1
    return f"{round(size, 2):,} {POWER_LABELS[n]}"


cpdef inline str pad_string(str string, int final_length, str pad=" "):
    """
    Return the given string front padded.

    Parameters
    ----------
    string : str
        The string to pad.
    final_length : int
        The final length to pad to.
    pad : str
        The padding character.

    Returns
    -------
    str

    """
    Condition.not_none(string, "string")
    Condition.not_negative_int(final_length, "length")
    Condition.not_none(pad, "pad")

    return ((final_length - len(string)) * pad) + string
