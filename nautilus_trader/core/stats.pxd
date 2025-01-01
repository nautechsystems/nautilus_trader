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

cimport numpy as np


cpdef double fast_mean(np.ndarray values)
cpdef double fast_mean_iterated(
    np.ndarray values,
    double next_value,
    double current_value,
    int expected_length,
    bint drop_left=*,
)
cpdef double fast_std(np.ndarray values)
cpdef double fast_std_with_mean(np.ndarray values, double mean)
cpdef double fast_mad(np.ndarray values)
cpdef double fast_mad_with_mean(np.ndarray values, double mean)
cpdef double basis_points_as_percentage(double basis_points)
