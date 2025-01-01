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

from nautilus_trader.indicators.average.moving_average cimport MovingAverage
from nautilus_trader.indicators.efficiency_ratio cimport EfficiencyRatio


cdef class AdaptiveMovingAverage(MovingAverage):
    cdef EfficiencyRatio _efficiency_ratio
    cdef double _prior_value

    cdef readonly int period_er
    """The period of the internal `EfficiencyRatio` indicator.\n\n:returns: `double`"""
    cdef readonly int period_alpha_fast
    """The period of the fast smoothing constant.\n\n:returns: `double`"""
    cdef readonly int period_alpha_slow
    """The period of the slow smoothing constant.\n\n:returns: `double`"""
    cdef readonly double alpha_fast
    """The alpha fast value.\n\n:returns: `double`"""
    cdef readonly double alpha_slow
    """The alpha slow value.\n\n:returns: `double`"""
    cdef readonly double alpha_diff
    """The alpha difference value.\n\n:returns: `double`"""
