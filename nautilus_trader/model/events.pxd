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

from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.core.message cimport Event
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.position cimport Position


cdef class AccountState(Event):
    cdef AccountId _account_id
    cdef Currency _currency
    cdef Money _balance
    cdef Money _margin_balance
    cdef Money _margin_available


cdef class OrderEvent(Event):
    cdef ClientOrderId _cl_ord_id
    cdef bint _is_completion_trigger


cdef class OrderInitialized(OrderEvent):
    cdef StrategyId _strategy_id
    cdef Symbol _symbol
    cdef OrderSide _order_side
    cdef OrderType _order_type
    cdef Quantity _quantity
    cdef TimeInForce _time_in_force
    cdef dict _options


cdef class OrderInvalid(OrderEvent):
    cdef str _reason


cdef class OrderDenied(OrderEvent):
    cdef str _reason


cdef class OrderSubmitted(OrderEvent):
    cdef AccountId _account_id
    cdef datetime _submitted_time


cdef class OrderRejected(OrderEvent):
    cdef AccountId _account_id
    cdef datetime _rejected_time
    cdef str _reason


cdef class OrderAccepted(OrderEvent):
    cdef AccountId _account_id
    cdef OrderId _order_id
    cdef datetime _accepted_time


cdef class OrderWorking(OrderEvent):
    cdef AccountId _account_id
    cdef OrderId _order_id
    cdef Symbol _symbol
    cdef OrderSide _order_side
    cdef OrderType _order_type
    cdef Quantity _quantity
    cdef Price _price
    cdef TimeInForce _time_in_force
    cdef datetime _expire_time
    cdef datetime _working_time


cdef class OrderCancelReject(OrderEvent):
    cdef AccountId _account_id
    cdef datetime _rejected_time
    cdef str _response_to
    cdef str _reason


cdef class OrderCancelled(OrderEvent):
    cdef AccountId _account_id
    cdef OrderId _order_id
    cdef datetime _cancelled_time


cdef class OrderModified(OrderEvent):
    cdef AccountId _account_id
    cdef OrderId _order_id
    cdef Quantity _modified_quantity
    cdef Price _modified_price
    cdef datetime _modified_time


cdef class OrderExpired(OrderEvent):
    cdef AccountId _account_id
    cdef OrderId _order_id
    cdef datetime _expired_time


cdef class OrderFilled(OrderEvent):
    cdef AccountId _account_id
    cdef OrderId _order_id
    cdef ExecutionId _execution_id
    cdef PositionId _position_id
    cdef StrategyId _strategy_id
    cdef Symbol _symbol
    cdef OrderSide _order_side
    cdef Quantity _filled_qty
    cdef Quantity _cumulative_qty
    cdef Quantity _leaves_qty
    cdef bint _is_partial_fill
    cdef Decimal _avg_price
    cdef Money _commission
    cdef LiquiditySide _liquidity_side
    cdef Currency _base_currency
    cdef Currency _quote_currency
    cdef bint _is_inverse
    cdef datetime _execution_time

    cdef OrderFilled clone(self, PositionId position_id, StrategyId strategy_id)


cdef class PositionEvent(Event):
    cdef Position _position
    cdef OrderFilled _order_fill


cdef class PositionOpened(PositionEvent):
    pass


cdef class PositionModified(PositionEvent):
    pass


cdef class PositionClosed(PositionEvent):
    pass
