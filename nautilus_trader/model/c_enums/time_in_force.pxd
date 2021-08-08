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


cpdef enum TimeInForce:
    DAY = 1,
    GTC = 2,  # Good till Canceled
    IOC = 3,  # Immediate or Cancel
    FOK = 4,  # Fill or Kill
    FAK = 5,  # Fill and Kill
    GTD = 6,  # Good till Date
    OC = 7,   # On Close


cdef class TimeInForceParser:

    @staticmethod
    cdef str to_str(int value)

    @staticmethod
    cdef TimeInForce from_str(str value) except *
