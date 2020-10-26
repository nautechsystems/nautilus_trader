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

    cdef readonly PositionId id
    cdef readonly AccountId account_id
    cdef readonly ClientOrderId from_order
    cdef readonly StrategyId strategy_id

    cdef readonly datetime timestamp
    cdef readonly Symbol symbol
    cdef readonly OrderSide entry
    cdef readonly PositionSide side
    cdef readonly Quantity quantity
    cdef readonly Quantity peak_quantity
    cdef readonly Currency base_currency
    cdef readonly Currency quote_currency
    cdef readonly datetime opened_time
    cdef readonly datetime closed_time
    cdef readonly timedelta open_duration
    cdef readonly Decimal avg_open
    cdef readonly Decimal avg_close
    cdef readonly Decimal realized_points
    cdef readonly Decimal realized_return
    cdef readonly Money realized_pnl
    cdef readonly Money commission
    cdef readonly QuoteTick last_tick

    @staticmethod
    cdef inline PositionSide side_from_order_side_c(OrderSide side) except *

    cdef str status_string(self)
    cpdef Money unrealized_pnl(self, QuoteTick last)
    cpdef Money total_pnl(self, QuoteTick last)
    cpdef void apply(self, OrderFilled event) except *
    cdef inline void _handle_buy_order_fill(self, OrderFilled event) except *
    cdef inline void _handle_sell_order_fill(self, OrderFilled event) except *
    cdef inline Decimal _calculate_cost(self, Decimal avg_price, Quantity total_quantity)
    cdef inline Decimal _calculate_avg_price(self, Decimal open_price, Quantity open_quantity, OrderFilled event)
    cdef inline Decimal _calculate_avg_open_price(self, OrderFilled event)
    cdef inline Decimal _calculate_avg_close_price(self, OrderFilled event)
    cdef inline Decimal _calculate_points(self, Decimal open_price, Decimal close_price)
    cdef inline Decimal _calculate_return(self, Decimal open_price, Decimal close_price)
    cdef inline Money _calculate_pnl(self, Decimal open_price, Decimal close_price, Quantity filled_qty)
