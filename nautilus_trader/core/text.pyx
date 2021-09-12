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

import cython

from libc.math cimport pow
from libc.stdint cimport uint8_t

from nautilus_trader.core.correctness cimport Condition


@cython.boundscheck(False)
@cython.wraparound(False)
cpdef inline uint8_t precision_from_str(str value) except *:
    """
    Return the decimal precision inferred from the given string.

    Can accept scientific notation strings including an 'e' character.

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
    If not scientific notation and no decimal point '.', then precision will be
    inferred as zero.

    """
    Condition.valid_string(value, "value")

    value = value.lower()
    if value.find("e-") > -1:
        # Scientific notation string
        return int(value.partition('e-')[2])
    else:
        # If does not contain "." then partition[2] will be ""
        return len(value.partition('.')[2])


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
