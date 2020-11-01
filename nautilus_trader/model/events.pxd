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
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly Currency currency
    """The currency of the event.\n\n:returns: `AccountId`"""
    cdef readonly Money balance
    """The account balance of the event.\n\n:returns: `AccountId`"""
    cdef readonly Money margin_balance
    """The margin balance of the event.\n\n:returns: `AccountId`"""
    cdef readonly Money margin_available
    """The margin available of the event.\n\n:returns: `AccountId`"""


cdef class OrderEvent(Event):
    cdef readonly ClientOrderId cl_ord_id
    """The client order identifier associated with the event.\n\n:returns: `ClientOrderId`"""
    cdef readonly bint is_completion_trigger
    """If this event represents an `Order` completion trigger.\n\n:returns: `bool`"""


cdef class OrderInitialized(OrderEvent):
    cdef readonly StrategyId strategy_id
    """The strategy identifier associated with the event.\n\n:returns: `StrategyId`"""
    cdef readonly Symbol symbol
    """The order symbol of the event.\n\n:returns: `Symbol`"""
    cdef readonly OrderSide order_side
    """The order side of the event.\n\n:returns: `OrderSide`"""
    cdef readonly OrderType order_type
    """The order type of the event.\n\n:returns: `OrderType`"""
    cdef readonly Quantity quantity
    """The order quantity of the event.\n\n:returns: `Quantity`"""
    cdef readonly TimeInForce time_in_force
    """The order time-in-force of the event.\n\n:returns: `TimeInForce`"""
    cdef readonly dict options
    """The order initialization options of the event.\n\n:returns: `dict`"""


cdef class OrderInvalid(OrderEvent):
    cdef readonly str reason
    """The reason the order was considered invalid.\n\n:returns: `str`"""


cdef class OrderDenied(OrderEvent):
    cdef readonly str reason
    """The reason the order was denied.\n\n:returns: `str`"""


cdef class OrderSubmitted(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly datetime submitted_time
    """The order submitted time of the event.\n\n:returns: `datetime`"""


cdef class OrderRejected(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly datetime rejected_time
    """The order rejected time of the event.\n\n:returns: `datetime`"""
    cdef readonly str reason
    """The reason the order was rejected.\n\n:returns: `str`"""


cdef class OrderAccepted(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly OrderId order_id
    """The order identifier associated with the event.\n\n:returns: `OrderId`"""
    cdef readonly datetime accepted_time
    """The order accepted time of the event.\n\n:returns: `datetime`"""


cdef class OrderWorking(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly OrderId order_id
    """The order identifier associated with the event.\n\n:returns: `OrderId`"""
    cdef readonly Symbol symbol
    """The order symbol of the event.\n\n:returns: `Symbol`"""
    cdef readonly OrderSide order_side
    """The order symbol of the event.\n\n:returns: `OrderSide`"""
    cdef readonly OrderType order_type
    """The order type of the event.\n\n:returns: `OrderType`"""
    cdef readonly Quantity quantity
    """The order quantity of the event.\n\n:returns: `Quantity`"""
    cdef readonly Price price
    """The order price of the event.\n\n:returns: `Price`"""
    cdef readonly TimeInForce time_in_force
    """The order time-in-force of the event.\n\n:returns: `TimeInForce`"""
    cdef readonly datetime expire_time
    """The order expire time of the event.\n\n:returns: `datetime`"""
    cdef readonly datetime working_time
    """The order working time of the event.\n\n:returns: `datetime`"""


cdef class OrderCancelReject(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly datetime rejected_time
    """The requests rejected time of the event.\n\n:returns: `datetime`"""
    cdef readonly str response_to
    """The cancel rejection response to.\n\n:returns: `str`"""
    cdef readonly str reason
    """The reason for order cancel rejection.\n\n:returns: `str`"""


cdef class OrderCancelled(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly OrderId order_id
    """The order identifier associated with the event.\n\n:returns: `OrderId`"""
    cdef readonly datetime cancelled_time
    """The order cancelled time of the event.\n\n:returns: `datetime`"""


cdef class OrderModified(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly OrderId order_id
    """The order identifier associated with the event.\n\n:returns: `OrderId`"""
    cdef readonly Quantity modified_quantity
    """The order quantity of the event.\n\n:returns: `Quantity`"""
    cdef readonly Price modified_price
    """The order price of the event.\n\n:returns: `Price`"""
    cdef readonly datetime modified_time
    """The order modified time of the event.\n\n:returns: `datetime`"""


cdef class OrderExpired(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly OrderId order_id
    """The order identifier associated with the event.\n\n:returns: `OrderId`"""
    cdef readonly datetime expired_time
    """The order expired time of the event.\n\n:returns: `datetime`"""


cdef class OrderFilled(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly OrderId order_id
    """The order identifier associated with the event.\n\n:returns: `OrderId`"""
    cdef readonly ExecutionId execution_id
    """The execution identifier associated with the event.\n\n:returns: `ExecutionId`"""
    cdef readonly PositionId position_id
    """The position identifier associated with the event.\n\n:returns: `PositionId`"""
    cdef readonly StrategyId strategy_id
    """The strategy identifier associated with the event.\n\n:returns: `StrategyId`"""
    cdef readonly Symbol symbol
    """The order symbol of the event.\n\n:returns: `Symbol`"""
    cdef readonly OrderSide order_side
    """The order side of the event.\n\n:returns: `OrderSide`"""
    cdef readonly Quantity filled_qty
    """The order filled quantity of the event.\n\n:returns: `Quantity`"""
    cdef readonly Quantity cumulative_qty
    """The order cumulative filled quantity.\n\n:returns: `Quantity`"""
    cdef readonly Quantity leaves_qty
    """The order quantity remaining to be filled.\n\n:returns: `Quantity`"""
    cdef readonly bint is_partial_fill
    """If the event represents a partial fill of the order.\n\n:returns: `bool`"""
    cdef readonly Decimal avg_price
    """The average fill price of the event.\n\n:returns: `Decimal`"""
    cdef readonly Money commission
    """The commission generated from the fill event.\n\n:returns: `Money`"""
    cdef readonly LiquiditySide liquidity_side
    """The liquidity side of the event (MAKER or TAKER).\n\n:returns: `LiquiditySide`"""
    cdef readonly Currency base_currency
    """The base currency of the event.\n\n:returns: `Currency`"""
    cdef readonly Currency quote_currency
    """The quote currency of the event.\n\n:returns: `Currency`"""
    cdef readonly bint is_inverse
    """If the instrument associated with the event is inverse.\n\n:returns: `bool`"""
    cdef readonly datetime execution_time
    """The execution timestamp of the event.\n\n:returns: `datetime`"""


cdef class PositionEvent(Event):
    cdef readonly Position position
    """The position associated with the event.\n\n:returns: `Position`"""
    cdef readonly OrderFilled order_fill
    """The order fill of the event.\n\n:returns: `OrderFilled`"""


cdef class PositionOpened(PositionEvent):
    pass


cdef class PositionModified(PositionEvent):
    pass


cdef class PositionClosed(PositionEvent):
    pass
