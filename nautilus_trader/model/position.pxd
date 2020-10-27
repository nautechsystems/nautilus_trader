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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick


cdef class Position:
    cdef list _events
    cdef Quantity _buy_quantity
    cdef Quantity _sell_quantity
    cdef Decimal _relative_quantity

    cdef PositionId _id
    cdef AccountId _account_id
    cdef ClientOrderId _from_order
    cdef StrategyId _strategy_id
    cdef datetime _timestamp
    cdef Symbol _symbol
    cdef OrderSide _entry
    cdef PositionSide _side
    cdef Quantity _quantity
    cdef Quantity _peak_quantity
    cdef Currency _base_currency
    cdef Currency _quote_currency
    cdef datetime _opened_time
    cdef datetime _closed_time
    cdef timedelta _open_duration
    cdef Decimal _avg_open
    cdef Decimal _avg_close
    cdef Decimal _realized_points
    cdef Decimal _realized_return
    cdef Money _realized_pnl
    cdef Money _commission

    @staticmethod
    cdef inline PositionSide side_from_order_side_c(OrderSide side) except *

    cpdef void apply(self, OrderFilled event) except *

    cdef str status_string(self)
    cpdef Money unrealized_pnl(self, QuoteTick last)
    cpdef Money total_pnl(self, QuoteTick last)
    cdef inline void _handle_buy_order_fill(self, OrderFilled event) except *
    cdef inline void _handle_sell_order_fill(self, OrderFilled event) except *
    cdef inline Decimal _calculate_cost(self, Decimal avg_price, Quantity total_quantity)
    cdef inline Decimal _calculate_avg_price(self, Decimal open_price, Quantity open_quantity, OrderFilled event)
    cdef inline Decimal _calculate_avg_open_price(self, OrderFilled event)
    cdef inline Decimal _calculate_avg_close_price(self, OrderFilled event)
    cdef inline Decimal _calculate_points(self, Decimal open_price, Decimal close_price)
    cdef inline Decimal _calculate_return(self, Decimal open_price, Decimal close_price)
    cdef inline Money _calculate_pnl(self, Decimal open_price, Decimal close_price, Quantity filled_qty)
