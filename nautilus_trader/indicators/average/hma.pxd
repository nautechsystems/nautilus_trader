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

from nautilus_trader.indicators.average.moving_average cimport MovingAverage


cdef class HullMovingAverage(MovingAverage):
    cdef int _period_sqrt
    cdef np.ndarray _w1
    cdef np.ndarray _w2
    cdef np.ndarray _w3
    cdef MovingAverage _ma1
    cdef MovingAverage _ma2
    cdef MovingAverage _ma3

    cdef np.ndarray _get_weights(self, int size)
