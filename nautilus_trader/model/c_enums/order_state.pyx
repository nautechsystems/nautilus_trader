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

cdef class OrderStateParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 1:
            return "INITIALIZED"
        elif value == 2:
            return "INVALID"
        elif value == 3:
            return "DENIED"
        elif value == 4:
            return "SUBMITTED"
        elif value == 5:
            return "ACCEPTED"
        elif value == 6:
            return "REJECTED"
        elif value == 7:
            return "CANCELLED"
        elif value == 8:
            return "EXPIRED"
        elif value == 9:
            return "TRIGGERED"
        elif value == 10:
            return "PARTIALLY_FILLED"
        elif value == 11:
            return "FILLED"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef OrderState from_str(str value) except *:
        if value == "INITIALIZED":
            return OrderState.INITIALIZED
        elif value == "INVALID":
            return OrderState.INVALID
        elif value == "DENIED":
            return OrderState.DENIED
        elif value == "SUBMITTED":
            return OrderState.SUBMITTED
        elif value == "ACCEPTED":
            return OrderState.ACCEPTED
        elif value == "REJECTED":
            return OrderState.REJECTED
        elif value == "CANCELLED":
            return OrderState.CANCELLED
        elif value == "EXPIRED":
            return OrderState.EXPIRED
        elif value == "TRIGGERED":
            return OrderState.TRIGGERED
        elif value == "PARTIALLY_FILLED":
            return OrderState.PARTIALLY_FILLED
        elif value == "FILLED":
            return OrderState.FILLED
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return OrderStateParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return OrderStateParser.from_str(value)
