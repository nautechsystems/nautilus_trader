# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    TICK = 1
    TICK_IMBALANCE = 2
    TICK_RUNS = 3
    VOLUME = 4
    VOLUME_IMBALANCE = 5
    VOLUME_RUNS = 6
    VALUE = 7
    VALUE_IMBALANCE = 8
    VALUE_RUNS = 9
    MILLISECOND = 10
    SECOND = 11
    MINUTE = 12
    HOUR = 13
    DAY = 14
    WEEK = 15
    MONTH = 16
