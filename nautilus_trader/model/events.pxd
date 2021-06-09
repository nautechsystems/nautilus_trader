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

from libc.stdint cimport int64_t

from nautilus_trader.core.message cimport Event
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.instrument_close_type cimport InstrumentCloseType
from nautilus_trader.model.c_enums.instrument_status cimport InstrumentStatus
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.time_in_force cimport TimeInForce
from nautilus_trader.model.c_enums.venue_status cimport VenueStatus
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.position cimport Position


cdef class AccountState(Event):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly AccountType account_type
    """The account type for the event.\n\n:returns: `AccountType`"""
    cdef readonly Currency base_currency
    """The account type for the event.\n\n:returns: `Currency` or None"""
    cdef readonly list balances
    """The account balances.\n\n:returns: `list[AccountBalance]`"""
    cdef readonly bint is_reported
    """If the state is reported from the exchange (otherwise system calculated).\n\n:returns: `bool`"""
    cdef readonly dict info
    """The additional implementation specific account information.\n\n:returns: `dict[str, object]`"""
    cdef readonly int64_t ts_updated_ns
    """The UNIX timestamp (nanos) when the account was updated.\n\n:returns: `int64`"""


cdef class OrderEvent(Event):
    cdef readonly ClientOrderId client_order_id
    """The client order identifier associated with the event.\n\n:returns: `ClientOrderId`"""
    cdef readonly VenueOrderId venue_order_id
    """The venue order identifier associated with the event.\n\n:returns: `VenueOrderId`"""


cdef class OrderInitialized(OrderEvent):
    cdef readonly InstrumentId instrument_id
    """The order instrument identifier.\n\n:returns: `InstrumentId`"""
    cdef readonly StrategyId strategy_id
    """The strategy identifier associated with the event.\n\n:returns: `StrategyId`"""
    cdef readonly OrderSide order_side
    """The order side.\n\n:returns: `OrderSide`"""
    cdef readonly OrderType order_type
    """The order type.\n\n:returns: `OrderType`"""
    cdef readonly Quantity quantity
    """The order quantity.\n\n:returns: `Quantity`"""
    cdef readonly TimeInForce time_in_force
    """The order time-in-force.\n\n:returns: `TimeInForce`"""
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
    cdef readonly int64_t ts_submitted_ns
    """The order submitted time.\n\n:returns: `int64`"""


cdef class OrderRejected(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly str reason
    """The reason the order was rejected.\n\n:returns: `str`"""
    cdef readonly int64_t ts_rejected_ns
    """The UNIX timestamp (nanos) when the order was rejected.\n\n:returns: `int64`"""


cdef class OrderAccepted(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly int64_t ts_accepted_ns
    """The UNIX timestamp (nanos) when the order was accepted.\n\n:returns: `int64`"""


cdef class OrderPendingReplace(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly int64_t ts_pending_ns
    """The timestamp from which the replace was pending.\n\n:returns: `int64`"""


cdef class OrderPendingCancel(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly int64_t ts_pending_ns
    """The timestamp from which the cancel was pending.\n\n:returns: `int64`"""


cdef class OrderUpdateRejected(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly str response_to
    """The update rejection response to.\n\n:returns: `str`"""
    cdef readonly str reason
    """The reason for order update rejection.\n\n:returns: `str`"""
    cdef readonly int64_t ts_rejected_ns
    """The UNIX timestamp (nanos) when the update was rejected.\n\n:returns: `int64`"""


cdef class OrderCancelRejected(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly str response_to
    """The cancel rejection response to.\n\n:returns: `str`"""
    cdef readonly str reason
    """The reason for order cancel rejection.\n\n:returns: `str`"""
    cdef readonly int64_t ts_rejected_ns
    """The UNIX timestamp (nanos) when the cancel was rejected.\n\n:returns: `int64`"""


cdef class OrderUpdated(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly Quantity quantity
    """The orders current quantity.\n\n:returns: `Quantity`"""
    cdef readonly Price price
    """The orders current price.\n\n:returns: `Price`"""
    cdef readonly int64_t ts_updated_ns
    """The UNIX timestamp (nanos) when the order was updated.\n\n:returns: `int64`"""


cdef class OrderCanceled(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly int64_t ts_canceled_ns
    """The UNIX timestamp (nanos) when the order was canceled.\n\n:returns: `int64`"""


cdef class OrderTriggered(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly int64_t ts_triggered_ns
    """The UNIX timestamp (nanos) when the order was triggered.\n\n:returns: `int64`"""


cdef class OrderExpired(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly int64_t ts_expired_ns
    """The UNIX timestamp (nanos) when the order expired.\n\n:returns: `int64`"""


cdef class OrderFilled(OrderEvent):
    cdef readonly AccountId account_id
    """The account identifier associated with the event.\n\n:returns: `AccountId`"""
    cdef readonly ExecutionId execution_id
    """The execution identifier associated with the event.\n\n:returns: `ExecutionId`"""
    cdef readonly PositionId position_id
    """The position identifier associated with the event.\n\n:returns: `PositionId`"""
    cdef readonly StrategyId strategy_id
    """The strategy identifier associated with the event.\n\n:returns: `StrategyId`"""
    cdef readonly InstrumentId instrument_id
    """The order instrument identifier.\n\n:returns: `InstrumentId`"""
    cdef readonly OrderSide order_side
    """The order side.\n\n:returns: `OrderSide`"""
    cdef readonly Quantity last_qty
    """The fill quantity.\n\n:returns: `Quantity`"""
    cdef readonly Price last_px
    """The fill price for this execution.\n\n:returns: `Price`"""
    cdef readonly Currency currency
    """The currency of the price.\n\n:returns: `Currency`"""
    cdef readonly Money commission
    """The commission generated from the fill.\n\n:returns: `Money`"""
    cdef readonly LiquiditySide liquidity_side
    """The liquidity side of the event (MAKER or TAKER).\n\n:returns: `LiquiditySide`"""
    cdef readonly int64_t ts_filled_ns
    """The UNIX timestamp (nanos) when the order was filled.\n\n:returns: `int64`"""
    cdef readonly dict info
    """The additional fill information.\n\n:returns: `dict[str, object]`"""

    cdef bint is_buy_c(self) except *
    cdef bint is_sell_c(self) except *


cdef class PositionEvent(Event):
    cdef readonly Position position
    """The position associated with the event.\n\n:returns: `Position`"""
    cdef readonly OrderFilled order_fill
    """The order fill associated with the event.\n\n:returns: `OrderFilled`"""


cdef class PositionOpened(PositionEvent):
    pass


cdef class PositionChanged(PositionEvent):
    pass


cdef class PositionClosed(PositionEvent):
    pass


cdef class StatusEvent(Event):
    pass


cdef class VenueStatusEvent(StatusEvent):
    cdef readonly Venue venue
    """The event venue.\n\n:returns: `Venue`"""
    cdef readonly VenueStatus status
    """The events venue status.\n\n:returns: `VenueStatus`"""


cdef class InstrumentStatusEvent(StatusEvent):
    cdef readonly InstrumentId instrument_id
    """The event instrument identifier.\n\n:returns: `InstrumentId`"""
    cdef readonly InstrumentStatus status
    """The events instrument status.\n\n:returns: `InstrumentStatus`"""


cdef class InstrumentClosePrice(Event):
    cdef readonly InstrumentId instrument_id
    """The event instrument identifier.\n\n:returns: `InstrumentId`"""
    cdef readonly Price close_price
    """The events close price.\n\n:returns: `Price`"""
    cdef readonly InstrumentCloseType close_type
    """The events close type.\n\n:returns: `InstrumentCloseType`"""
