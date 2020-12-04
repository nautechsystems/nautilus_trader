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


cpdef enum ComponentTrigger:
    UNDEFINED = 0,  # Invalid value
    START = 1,
    RUNNING = 2,
    STOP = 3,
    STOPPED = 4,
    RESUME = 5,
    RESET = 6,
    DISPOSE = 7,
    DISPOSED = 8,


cdef class ComponentTriggerParser:

    @staticmethod
    cdef str to_str(int value)

    @staticmethod
    cdef ComponentTrigger from_str(str value)
