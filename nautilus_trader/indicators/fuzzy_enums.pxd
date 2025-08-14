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

# Consolidated fuzzy enums from fuzzy_enums/ subdirectory

cpdef enum CandleDirection:
    DIRECTION_BEAR = -1
    DIRECTION_NONE = 0  # Doji
    DIRECTION_BULL = 1


cpdef enum CandleSize:
    SIZE_NONE = 0  # Doji
    SIZE_VERY_SMALL = 1
    SIZE_SMALL = 2
    SIZE_MEDIUM = 3
    SIZE_LARGE = 4
    SIZE_VERY_LARGE = 5
    SIZE_EXTREMELY_LARGE = 6


cpdef enum CandleBodySize:
    BODY_NONE = 0  # Doji
    BODY_SMALL = 1
    BODY_MEDIUM = 2
    BODY_LARGE = 3
    BODY_TREND = 4


cpdef enum CandleWickSize:
    WICK_NONE = 0  # No candle wick
    WICK_SMALL = 1
    WICK_MEDIUM = 2
    WICK_LARGE = 3
