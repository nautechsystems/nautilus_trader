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

import pandas as pd


cdef int precision_from_string(str value)
cpdef double fast_mean(list values)
cpdef double fast_mean_iterated(
    list values,
    double next_value,
    double current_value,
    int expected_length,
    bint drop_left=*,
)
cpdef double fast_std(list values)
cpdef double fast_std_with_mean(list values, double mean)
cpdef double basis_points_as_percentage(double basis_points)
cdef long get_size_of(obj)
cpdef str format_bytes(double size)
cpdef str pad_string(str string, int length, str pad=*)


# Method only exists due to cython limitation compiling closures
cdef inline object slice_dataframe(dataframe, start, end):
    """
    Slice the dataframe with the given start and end.

    Parameters
    ----------
    dataframe : pd.DataFrame
        The dataframe to slice.
    start : should correspond to the index
        The start of the slice.
    end: should correspond to the index
        The end of the slice.

    Returns
    -------
    pd.DataFrame.
        The sliced data frame.

    """
    if dataframe is None:
        return pd.DataFrame()

    return dataframe[start:end]
