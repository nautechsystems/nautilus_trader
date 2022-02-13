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


cdef class OrderStatusParser:

    @staticmethod
    cdef str to_str(int value):
        if value == 1:
            return "INITIALIZED"
        elif value == 2:
            return "DENIED"
        elif value == 3:
            return "SUBMITTED"
        elif value == 4:
            return "ACCEPTED"
        elif value == 5:
            return "REJECTED"
        elif value == 6:
            return "CANCELED"
        elif value == 7:
            return "EXPIRED"
        elif value == 8:
            return "TRIGGERED"
        elif value == 9:
            return "PENDING_UPDATE"
        elif value == 10:
            return "PENDING_CANCEL"
        elif value == 11:
            return "PARTIALLY_FILLED"
        elif value == 12:
            return "FILLED"
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    cdef OrderStatus from_str(str value) except *:
        if value == "INITIALIZED":
            return OrderStatus.INITIALIZED
        elif value == "DENIED":
            return OrderStatus.DENIED
        elif value == "SUBMITTED":
            return OrderStatus.SUBMITTED
        elif value == "ACCEPTED":
            return OrderStatus.ACCEPTED
        elif value == "REJECTED":
            return OrderStatus.REJECTED
        elif value == "CANCELED":
            return OrderStatus.CANCELED
        elif value == "EXPIRED":
            return OrderStatus.EXPIRED
        elif value == "TRIGGERED":
            return OrderStatus.TRIGGERED
        elif value == "PENDING_UPDATE":
            return OrderStatus.PENDING_UPDATE
        elif value == "PENDING_CANCEL":
            return OrderStatus.PENDING_CANCEL
        elif value == "PARTIALLY_FILLED":
            return OrderStatus.PARTIALLY_FILLED
        elif value == "FILLED":
            return OrderStatus.FILLED
        else:
            raise ValueError(f"value was invalid, was {value}")

    @staticmethod
    def to_str_py(int value):
        return OrderStatusParser.to_str(value)

    @staticmethod
    def from_str_py(str value):
        return OrderStatusParser.from_str(value)
