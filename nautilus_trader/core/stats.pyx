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

import cython

cimport numpy as np
from libc.math cimport sqrt


@cython.boundscheck(False)
@cython.wraparound(False)
cpdef double fast_mean(np.ndarray values):
    """
    Return the average value for numpy.ndarray values.

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
        raise ValueError("values must be valid numpy.ndarray with ndim == 1")

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
):
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
        raise ValueError("values must be valid ndarray with ndim == 1")

    cdef double[:] mv = values
    cdef int length = len(mv)

    if length < expected_length:
        return fast_mean(values)

    assert length == expected_length

    cdef double value_to_drop = mv[0] if drop_left else mv[length - 1]
    return current_value + (next_value - value_to_drop) / length


cpdef inline double fast_std(np.ndarray values):
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
cpdef double fast_std_with_mean(np.ndarray values, double mean):
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
        raise ValueError("values must be valid ndarray with ndim == 1")

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


cpdef inline double fast_mad(np.ndarray values):
    """
    Return the mean absolute deviation from the given values.

    Parameters
    ----------
    values : numpy.ndarray
        The array for the calculation.

    Returns
    -------
    double

    """
    return fast_mad_with_mean(values, fast_mean(values))


@cython.boundscheck(False)
@cython.wraparound(False)
cpdef double fast_mad_with_mean(np.ndarray values, double mean):
    """
    Return the mean absolute deviation from the given values and mean.

    Parameters
    ----------
    values : numpy.ndarray
        The array for the calculation.
    mean : double
        The pre-calculated mean of the given values.

    Returns
    -------
    double

    """
    if values is None or values.ndim != 1:
        raise ValueError("values must be valid ndarray with ndim == 1")

    cdef double[:] mv = values
    cdef int length = len(mv)

    if length == 0:
        return 0.0

    cdef double mad = 0.0
    cdef double v
    cdef int i
    with nogil:
        for i in range(length):
            v = abs(mv[i] - mean)
            mad += v

    return mad / length


cpdef inline double basis_points_as_percentage(double basis_points):
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
