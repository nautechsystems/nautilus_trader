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


cpdef enum OrderType:
    MARKET = 1
    LIMIT = 2
    STOP_MARKET = 3
    STOP_LIMIT = 4
    MARKET_TO_LIMIT = 5
    MARKET_IF_TOUCHED = 6
    LIMIT_IF_TOUCHED = 7
    TRAILING_STOP_MARKET = 8
    TRAILING_STOP_LIMIT = 9


cdef class OrderTypeParser:

    @staticmethod
    cdef str to_str(int value)

    @staticmethod
    cdef OrderType from_str(str value) except *
