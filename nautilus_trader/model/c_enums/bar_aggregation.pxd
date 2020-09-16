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


cpdef enum BarAggregation:
    UNDEFINED = 0,  # Invalid value
    TICK = 1,
    TICK_IMBALANCE = 2,
    VOLUME = 3,
    VOLUME_IMBALANCE = 4,
    DOLLAR = 5,
    DOLLAR_IMBALANCE = 6
    SECOND = 7,
    MINUTE = 8,
    HOUR = 9,
    DAY = 10,


cdef inline str bar_aggregation_to_string(int value):
    if value == 1:
        return 'TICK'
    elif value == 2:
        return 'TICK_IMBALANCE'
    elif value == 3:
        return 'VOLUME'
    elif value == 4:
        return 'VOLUME_IMBALANCE'
    elif value == 5:
        return 'DOLLAR'
    elif value == 6:
        return 'DOLLAR_IMBALANCE'
    elif value == 7:
        return 'SECOND'
    elif value == 8:
        return 'MINUTE'
    elif value == 9:
        return 'HOUR'
    elif value == 10:
        return 'DAY'
    else:
        return 'UNDEFINED'


cdef inline BarAggregation bar_aggregation_from_string(str value):
    if value == 'TICK':
        return BarAggregation.TICK
    elif value == 'TICK_IMBALANCE':
        return BarAggregation.TICK_IMBALANCE
    elif value == 'VOLUME':
        return BarAggregation.VOLUME
    elif value == 'VOLUME_IMBALANCE':
        return BarAggregation.VOLUME_IMBALANCE
    elif value == 'DOLLAR':
        return BarAggregation.DOLLAR
    elif value == 'DOLLAR_IMBALANCE':
        return BarAggregation.DOLLAR_IMBALANCE
    elif value == 'SECOND':
        return BarAggregation.SECOND
    elif value == 'MINUTE':
        return BarAggregation.MINUTE
    elif value == 'HOUR':
        return BarAggregation.HOUR
    elif value == 'DAY':
        return BarAggregation.DAY
    else:
        return BarAggregation.UNDEFINED
