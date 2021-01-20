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


cpdef enum OrderState:
    UNDEFINED = 0,  # Invalid value
    INITIALIZED = 1,
    INVALID = 2,
    DENIED = 3,
    SUBMITTED = 4,
    ACCEPTED = 5,
    REJECTED = 6,
    CANCELLED = 7,
    EXPIRED = 8,
    PARTIALLY_FILLED = 9,
    FILLED = 10


cdef class OrderStateParser:

    @staticmethod
    cdef str to_str(int value)

    @staticmethod
    cdef OrderState from_str(str value)
