# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.unicode cimport PyUnicode_FromString
from libc.stdint cimport uint8_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport cstring_free


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


cdef inline str cstr_to_pystr(const char* data):
    cdef str obj = PyUnicode_FromString(data)
    cstring_free(data)
    return obj
