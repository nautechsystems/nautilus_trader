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
    """
    Returns
    -------
    AccountId
        The account identifier associated with the event.

    """

    cdef readonly Currency currency
    """
    Returns
    -------
    Currency
        The currency of the event.

    """

    cdef readonly Money balance
    """
    Returns
    -------
    Money
        The account balance of the event.

    """

    cdef readonly Money margin_balance
    """
    Returns
    -------
    Money
        The margin balance of the event.

    """

    cdef readonly Money margin_available
    """
    Returns
    -------
    Money
        The margin available of the event.

    """


cdef class OrderEvent(Event):
    cdef readonly ClientOrderId cl_ord_id
    """
    Returns
    -------
    ClientOrderId
        The client order identifier associated with the event.

    """

    cdef readonly bint is_completion_trigger
    """
    Returns
    -------
    bool
        If this event represents an `Order` completion trigger (where an order
        will subsequently be considered `completed` when this event is applied).

    """


cdef class OrderInitialized(OrderEvent):
    cdef readonly StrategyId strategy_id
    """
    Returns
    -------
    StrategyId
        The strategy identifier associated with the event.

    """

    cdef readonly Symbol symbol
    """
    Returns
    -------
    Symbol
        The order symbol of the event.

    """

    cdef readonly OrderSide order_side
    """
    Returns
    -------
    OrderSide
        The order side of the event.

    """

    cdef readonly OrderType order_type
    """
    Returns
    -------
    OrderType
        The order type of the event.

    """

    cdef readonly Quantity quantity
    """
    Returns
    -------
    Quantity
        The order quantity of the event.

    """

    cdef readonly TimeInForce time_in_force
    """
    Returns
    -------
    TimeInForce
        The order time-in-force of the event.

    """

    cdef readonly dict options
    """
    Returns
    -------
    dict
        The order initialization options of the event.

    """


cdef class OrderInvalid(OrderEvent):
    cdef readonly str reason
    """
    Returns
    -------
    str
        The reason the order was considered invalid.

    """


cdef class OrderDenied(OrderEvent):
    cdef readonly str reason
    """
    Returns
    -------
    str
        The reason the order was denied.

    """


cdef class OrderSubmitted(OrderEvent):
    cdef readonly AccountId account_id
    """
    Returns
    -------
    AccountId
        The account identifier associated with the event.

    """

    cdef readonly datetime submitted_time
    """
    Returns
    -------
    datetime
        The order submitted time of the event.

    """


cdef class OrderRejected(OrderEvent):
    cdef readonly AccountId account_id
    """
    Returns
    -------
    AccountId
        The account identifier associated with the event.

    """

    cdef readonly datetime rejected_time
    """
    Returns
    -------
    datetime
        The order rejected time of the event.

    """

    cdef readonly str reason
    """
    Returns
    -------
    str
        The reason the order was rejected.

    """


cdef class OrderAccepted(OrderEvent):
    cdef readonly AccountId account_id
    """
    Returns
    -------
    AccountId
        The account identifier associated with the event.

    """

    cdef readonly OrderId order_id
    """
    Returns
    -------
    OrderId
        The order identifier associated with the event.

    """

    cdef readonly datetime accepted_time
    """
    Returns
    -------
    datetime
        The order accepted time of the event.

    """


cdef class OrderWorking(OrderEvent):
    cdef readonly AccountId account_id
    """
    Returns
    -------
    AccountId
        The account identifier associated with the event.

    """

    cdef readonly OrderId order_id
    """
    Returns
    -------
    OrderId
        The order identifier associated with the event.

    """

    cdef readonly Symbol symbol
    """
    Returns
    -------
    Symbol
        The order symbol of the event.

    """

    cdef readonly OrderSide order_side
    """
    Returns
    -------
    datetime
        The order symbol of the event.

    """

    cdef readonly OrderType order_type
    """
    Returns
    -------
    OrderType
        The order type of the event.

    """

    cdef readonly Quantity quantity
    """
    Returns
    -------
    Quantity
        The order quantity of the event.

    """

    cdef readonly Price price
    """
    Returns
    -------
    Price
        The order price of the event.

    """

    cdef readonly TimeInForce time_in_force
    """
    Returns
    -------
    TimeInForce
        The order time-in-force of the event.

    """

    cdef readonly datetime expire_time
    """
    Returns
    -------
    datetime or None
        The order expire time of the event.

    """

    cdef readonly datetime working_time
    """
    Returns
    -------
    datetime
        The order working time of the event.

    """


cdef class OrderCancelReject(OrderEvent):
    cdef readonly AccountId account_id
    """
    Returns
    -------
    AccountId
        The account identifier associated with the event.

    """

    cdef readonly datetime rejected_time
    """
    Returns
    -------
    datetime
        The requests rejected time of the event.

    """

    cdef readonly str response_to
    """
    Returns
    -------
    str
        The cancel rejection response to.

    """

    cdef readonly str reason
    """
    Returns
    -------
    str
        The reason for order cancel rejection.

    """


cdef class OrderCancelled(OrderEvent):
    cdef readonly AccountId account_id
    """
    Returns
    -------
    AccountId
        The account identifier associated with the event.

    """

    cdef readonly OrderId order_id
    """
    Returns
    -------
    OrderId
        The order identifier associated with the event.

    """

    cdef readonly datetime cancelled_time
    """
    Returns
    -------
    datetime
        The order cancelled time of the event.

    """


cdef class OrderExpired(OrderEvent):
    cdef readonly AccountId account_id
    """
    Returns
    -------
    AccountId
        The account identifier associated with the event.

    """

    cdef readonly OrderId order_id
    """
    Returns
    -------
    OrderId
        The order identifier associated with the event.

    """

    cdef readonly datetime expired_time
    """
    Returns
    -------
    datetime
        The order expired time of the event.

    """


cdef class OrderModified(OrderEvent):
    cdef readonly AccountId account_id
    """
    Returns
    -------
    AccountId
        The account identifier associated with the event.

    """

    cdef readonly OrderId order_id
    """
    Returns
    -------
    OrderId
        The order identifier associated with the event.

    """

    cdef readonly Quantity modified_quantity
    """
    Returns
    -------
    Quantity
        The order quantity of the event.

    """

    cdef readonly Price modified_price
    """
    Returns
    -------
    Price
        The order price of the event.

    """

    cdef readonly datetime modified_time
    """
    Returns
    -------
    datetime
        The order modified time of the event.

    """


cdef class OrderFilled(OrderEvent):
    cdef readonly AccountId account_id
    """
    Returns
    -------
    AccountId
        The account identifier associated with the event.

    """

    cdef readonly OrderId order_id
    """
    Returns
    -------
    OrderId
        The order identifier associated with the event.

    """

    cdef readonly ExecutionId execution_id
    """
    Returns
    -------
    ExecutionId
        The execution identifier associated with the event.

    """

    cdef readonly PositionId position_id
    """
    Returns
    -------
    PositionId
        The position identifier associated with the event.

    """

    cdef readonly StrategyId strategy_id
    """
    Returns
    -------
    StrategyId
        The strategy identifier associated with the event.

    """

    cdef readonly Symbol symbol
    """
    Returns
    -------
    Symbol
        The order symbol of the event.

    """

    cdef readonly OrderSide order_side
    """
    Returns
    -------
    OrderSide
        The order side of the event.

    """

    cdef readonly Quantity filled_qty
    """
    Returns
    -------
    Quantity
        The order filled quantity of the event.

    """

    cdef readonly Quantity cumulative_qty
    """
    Returns
    -------
    Quantity
        The cumulative filled quantity of the order.

    """

    cdef readonly Quantity leaves_qty
    """
    Returns
    -------
    Quantity
        The quantity quantity remaining to be filled of the order.

    """

    cdef readonly bint is_partial_fill
    """
    Returns
    -------
    bool
        If the event represents a partial fill of the order.

    """

    cdef readonly Price avg_price
    """
    Returns
    -------
    Price
        The average fill price of the event.

    """

    cdef readonly Money commission
    """
    Returns
    -------
    Money
        The commission generated from the fill event.

    """

    cdef readonly LiquiditySide liquidity_side
    """
    Returns
    -------
    LiquiditySide
        The liquidity side of the event (if the order was MAKER or TAKER).

    """

    cdef readonly Currency base_currency
    """
    Returns
    -------
    Currency
        The base currency of the event.

    """

    cdef readonly Currency quote_currency
    """
    Returns
    -------
    Currency
        The quote currency of the event.

    """

    cdef readonly bint is_inverse
    """
    Returns
    -------
    bool
        If the instrument associated with the event is inverse.

    """

    cdef readonly datetime execution_time
    """
    Returns
    -------
    datetime
        The execution timestamp of the event.

    """

    cdef OrderFilled clone(self, PositionId position_id, StrategyId strategy_id)


cdef class PositionEvent(Event):
    cdef readonly Position position
    """
    Returns
    -------
    Position
        The position associated with the event.

    """

    cdef readonly OrderFilled order_fill
    """
    Returns
    -------
    OrderFilled
        The order fill of the event.

    """


cdef class PositionOpened(PositionEvent):
    pass


cdef class PositionModified(PositionEvent):
    pass


cdef class PositionClosed(PositionEvent):
    pass
