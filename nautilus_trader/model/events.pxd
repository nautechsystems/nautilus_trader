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

from cpython.datetime cimport datetime

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
    cdef readonly list balances
    """The currency account balances.\n\n:returns: `list[Money]`"""
    cdef readonly list balances_free
    """The account balances free for trading.\n\n:returns: `list[Money]`"""
    cdef readonly list balances_locked
    """The account balances locked (assigned to pending orders).\n\n:returns: `list[Money]`"""
    cdef readonly dict info
    """The additional implementation specific account information.\n\n:returns: `dict[str, object]`"""


cdef class OrderEvent(Event):
    cdef readonly ClientOrderId cl_ord_id
    """The client order identifier associated with the event.\n\n:returns: `ClientOrderId`"""
    cdef readonly OrderId order_id
    """The order identifier associated with the event.\n\n:returns: `OrderId`"""


cdef class OrderInitialized(OrderEvent):
    cdef readonly Symbol symbol
    """The order symbol.\n\n:returns: `Symbol`"""
    cdef readonly StrategyId strategy_id
    """The strategy identifier associated with the event.\n\n:returns: `StrategyId`"""
    cdef readonly OrderSide order_side
    """The order side.\n\n:returns: `OrderSide` (Enum)"""
    cdef readonly OrderType order_type
    """The order type.\n\n:returns: `OrderType` (Enum)"""
    cdef readonly Quantity quantity
    """The order quantity.\n\n:returns: `Quantity`"""
    cdef readonly TimeInForce time_in_force
    """The order time-in-force.\n\n:returns: `TimeInForce` (Enum)"""
    cdef readonly dict options
    """The order initialization options.\n\n:returns: `dict`"""


cdef class OrderInvalid(OrderEvent):
    cdef readonly str reason
    """The reason the order was invalid.\n\n:returns: `str`"""


cdef class OrderDenied(OrderEvent):
    cdef readonly str reason
    """The reason the order was denied.\n\n:returns: `str`"""


cdef class OrderSubmitted(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly datetime submitted_time
    """The order submitted time.\n\n:returns: `datetime`"""
    cdef readonly unsigned long latency
    """The latency from order initialization to submission.\n\n:returns: `long`"""


cdef class OrderRejected(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly datetime rejected_time
    """The order rejected time.\n\n:returns: `datetime`"""
    cdef readonly str reason
    """The reason the order was rejected.\n\n:returns: `str`"""


cdef class OrderAccepted(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly datetime accepted_time
    """The order accepted time.\n\n:returns: `datetime`"""


cdef class OrderWorking(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly Symbol symbol
    """The order symbol.\n\n:returns: `Symbol`"""
    cdef readonly OrderSide order_side
    """The order side.\n\n:returns: `OrderSide` (Enum)"""
    cdef readonly OrderType order_type
    """The order type.\n\n:returns: `OrderType` (Enum)"""
    cdef readonly Quantity quantity
    """The order quantity.\n\n:returns: `Quantity`"""
    cdef readonly Price price
    """The order price (STOP or LIMIT).\n\n:returns: `Price`"""
    cdef readonly TimeInForce time_in_force
    """The order time-in-force.\n\n:returns: `TimeInForce` (Enum)"""
    cdef readonly datetime expire_time
    """The order expire time.\n\n:returns: `datetime` or `None`"""
    cdef readonly datetime working_time
    """The order working.\n\n:returns: `datetime`"""


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
    cdef readonly datetime cancelled_time
    """The order cancelled time.\n\n:returns: `datetime`"""


cdef class OrderAmended(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly Quantity quantity
    """The orders current quantity.\n\n:returns: `Quantity`"""
    cdef readonly Price price
    """The orders current price.\n\n:returns: `Price`"""
    cdef readonly datetime amended_time
    """The order amended time.\n\n:returns: `datetime`"""


cdef class OrderExpired(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly datetime expired_time
    """The order expired time.\n\n:returns: `datetime`"""


cdef class OrderFilled(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly ExecutionId execution_id
    """The execution identifier associated with the event.\n\n:returns: `ExecutionId`"""
    cdef readonly PositionId position_id
    """The position identifier associated with the event.\n\n:returns: `PositionId`"""
    cdef readonly StrategyId strategy_id
    """The strategy identifier associated with the event.\n\n:returns: `StrategyId`"""
    cdef readonly Symbol symbol
    """The order symbol.\n\n:returns: `Symbol`"""
    cdef readonly OrderSide order_side
    """The order side.\n\n:returns: `OrderSide` (Enum)"""
    cdef readonly Quantity fill_qty
    """The fill quantity.\n\n:returns: `Quantity`"""
    cdef readonly Quantity cum_qty
    """The order cumulative filled quantity.\n\n:returns: `Quantity`"""
    cdef readonly Quantity leaves_qty
    """The order quantity remaining to be filled.\n\n:returns: `Quantity`"""
    cdef readonly Price fill_price
    """The fill price for this execution.\n\n:returns: `Price`"""
    cdef readonly Currency currency
    """The currency of the price.\n\n:returns: `Currency`"""
    cdef readonly bint is_inverse
    """If quantity is expressed in quote currency.\n\n:returns: `bool`"""
    cdef readonly Money commission
    """The commission generated from the fill.\n\n:returns: `Money`"""
    cdef readonly LiquiditySide liquidity_side
    """The liquidity side of the event (MAKER or TAKER).\n\n:returns: `LiquiditySide` (Enum)"""
    cdef readonly datetime execution_time
    """The execution timestamp.\n\n:returns: `datetime`"""
    cdef readonly dict info
    """The additional fill information.\n\n:returns: `dict[str, object]`"""


cdef class PositionEvent(Event):
    cdef readonly Position position
    """The position associated with the event.\n\n:returns: `Position`"""
    cdef readonly OrderFilled order_fill
    """The order fill associated with the position.\n\n:returns: `OrderFilled`"""


cdef class PositionOpened(PositionEvent):
    pass


cdef class PositionChanged(PositionEvent):
    pass


cdef class PositionClosed(PositionEvent):
    pass
