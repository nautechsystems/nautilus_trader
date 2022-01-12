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


cpdef enum OrderStatus:
    INITIALIZED = 1
    DENIED = 2
    SUBMITTED = 3
    ACCEPTED = 4
    REJECTED = 5
    CANCELED = 6
    EXPIRED = 7
    TRIGGERED = 8
    PENDING_UPDATE = 9
    PENDING_CANCEL = 10
    PARTIALLY_FILLED = 11
    FILLED = 12


cdef class OrderStatusParser:

    @staticmethod
    cdef str to_str(int value)

    @staticmethod
    cdef OrderStatus from_str(str value) except *
