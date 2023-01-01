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

from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport TrailingOffsetType
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order


cdef class TrailingStopCalculator:

    @staticmethod
    cdef tuple calculate(
        Instrument instrument,
        Order order,
        Price bid,
        Price ask,
        Price last,
    )

    @staticmethod
    cdef Price calculate_with_last(
        Instrument instrument,
        TrailingOffsetType trailing_offset_type,
        OrderSide side,
        double offset,
        Price last,
    )

    @staticmethod
    cdef Price calculate_with_bid_ask(
        Instrument instrument,
        TrailingOffsetType trailing_offset_type,
        OrderSide side,
        double offset,
        Price bid,
        Price ask,
    )
