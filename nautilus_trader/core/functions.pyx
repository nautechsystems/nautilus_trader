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

import gc
import sys

from libc.math cimport pow
from libc.math cimport sqrt

from nautilus_trader.core.correctness cimport Condition


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


cpdef double fast_std(list values):
    """
    Return the standard deviation from the given values.

    :param values: The values for the calculation.
    :return: double.
    """
    return fast_std_with_mean(values, fast_mean(values))


cpdef double fast_std_with_mean(list values, double mean):
    """
    Return the standard deviation from the given values and mean.
    Note - garbage in garbage out for given mean.

    :param values: The values for the calculation.
    :param mean: The pre-calculated mean of the given values.
    :return: double.
    """
    cdef int length = len(values)
    cdef double std_dev = 0.0

    for i in range(length):
        std_dev += pow(values[i] - mean, 2)

    return sqrt(std_dev / length)


cpdef double basis_points_as_percentage(double basis_points):
    """
    Return the given basis points expressed as a percentage where 100% = 1.0.

    :param basis_points: The basis points to convert to percentage.
    :return double.
    """
    return basis_points * 0.0001


# Closures in cpdef functions not yet supported (10/02/20)
cdef long get_size_of(obj):
    Condition.not_none(obj, "obj")

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
    0: "bytes",
    1: "KB",
    2: "MB",
    3: "GB",
    4: "TB"
}

cpdef str format_bytes(double size):
    """
    Return the formatted bytes size.

    :param size: The size in bytes.
    :return: str.
    """
    Condition.not_negative(size, "size")

    cdef double power = pow(2, 10)

    cdef int n = 0
    while size >= power:
        size /= power
        n += 1
    return f"{round(size, 2):,} {POWER_LABELS[n]}"


cpdef str pad_string(str string, int length, str pad=" "):
    """
    Return the given string front padded.

    :param string: The string to pad.
    :param length: The length to pad to.
    :param pad: The padding character.

    :return str.

    """
    Condition.not_none(string, "string")
    Condition.not_negative_int(length, "length")
    Condition.not_none(pad, "pad")

    return ((length - len(string)) * pad) + string


# Closures in cpdef functions not yet supported (21/6/19)
def max_in_dict(dict dictionary):
    """
    Return the key for the maximum value held in the given dictionary.

    :param dictionary: The dictionary to check.
    :return The key.
    """
    Condition.not_none(dictionary, "dictionary")

    return max(dictionary.items(), key=lambda x: x[1])[0]
