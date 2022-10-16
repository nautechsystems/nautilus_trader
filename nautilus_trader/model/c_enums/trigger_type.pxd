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


cpdef enum TriggerType:
    NONE = 0
    DEFAULT = 1
    BID_ASK = 2
    LAST = 3
    DOUBLE_LAST = 4
    DOUBLE_BID_ASK = 5
    LAST_OR_BID_ASK = 6
    MID_POINT = 7
    MARK = 8
    INDEX = 9



cdef class TriggerTypeParser:

    @staticmethod
    cdef str to_str(int value)

    @staticmethod
    cdef TriggerType from_str(str value) except *
