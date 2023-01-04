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

from nautilus_trader.core.message cimport Event
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.enums_c cimport ContingencyType
from nautilus_trader.model.enums_c cimport LiquiditySide
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.enums_c cimport OrderType
from nautilus_trader.model.enums_c cimport TimeInForce
from nautilus_trader.model.enums_c cimport TriggerType
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class OrderEvent(Event):
    cdef readonly TraderId trader_id
    """The trader ID associated with the event.\n\n:returns: `TraderId`"""
    cdef readonly StrategyId strategy_id
    """The strategy ID associated with the event.\n\n:returns: `StrategyId`"""
    cdef readonly InstrumentId instrument_id
    """The instrument ID associated with the event.\n\n:returns: `InstrumentId`"""
    cdef readonly ClientOrderId client_order_id
    """The client order ID associated with the event.\n\n:returns: `ClientOrderId`"""
    cdef readonly VenueOrderId venue_order_id
    """The venue order ID associated with the event.\n\n:returns: `VenueOrderId` or ``None``"""
    cdef readonly AccountId account_id
    """The account ID associated with the event.\n\n:returns: `AccountId` or ``None``"""
    cdef readonly bint reconciliation
    """If the event was generated during reconciliation.\n\n:returns: `bool`"""


cdef class OrderInitialized(OrderEvent):
    cdef readonly OrderSide side
    """The order side.\n\n:returns: `OrderSide`"""
    cdef readonly OrderType order_type
    """The order type.\n\n:returns: `OrderType`"""
    cdef readonly Quantity quantity
    """The order quantity.\n\n:returns: `Quantity`"""
    cdef readonly TimeInForce time_in_force
    """The order time in force.\n\n:returns: `TimeInForce`"""
    cdef readonly bint post_only
    """If the order will only provide liquidity (make a market).\n\n:returns: `bool`"""
    cdef readonly bint reduce_only
    """If the order carries the 'reduce-only' execution instruction.\n\n:returns: `bool`"""
    cdef readonly dict options
    """The order initialization options.\n\n:returns: `dict`"""
    cdef readonly TriggerType emulation_trigger
    """The order emulation trigger type.\n\n:returns: `TriggerType`"""
    cdef readonly ContingencyType contingency_type
    """The orders contingency type.\n\n:returns: `ContingencyType`"""
    cdef readonly OrderListId order_list_id
    """The order list ID associated with the order.\n\n:returns: `OrderListId` or ``None``"""
    cdef readonly list linked_order_ids
    """The orders linked client order ID(s).\n\n:returns: `list[ClientOrderId]` or ``None``"""
    cdef readonly ClientOrderId parent_order_id
    """The orders parent client order ID.\n\n:returns: `ClientOrderId` or ``None``"""
    cdef readonly str tags
    """The order custom user tags.\n\n:returns: `str` or ``None``"""

    @staticmethod
    cdef OrderInitialized from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderInitialized obj)


cdef class OrderDenied(OrderEvent):
    cdef readonly str reason
    """The reason the order was denied.\n\n:returns: `str`"""

    @staticmethod
    cdef OrderDenied from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderDenied obj)


cdef class OrderSubmitted(OrderEvent):

    @staticmethod
    cdef OrderSubmitted from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderSubmitted obj)


cdef class OrderAccepted(OrderEvent):

    @staticmethod
    cdef OrderAccepted from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderAccepted obj)


cdef class OrderRejected(OrderEvent):
    cdef readonly str reason
    """The reason the order was rejected.\n\n:returns: `str`"""

    @staticmethod
    cdef OrderRejected from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderRejected obj)


cdef class OrderCanceled(OrderEvent):

    @staticmethod
    cdef OrderCanceled from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderCanceled obj)


cdef class OrderExpired(OrderEvent):

    @staticmethod
    cdef OrderExpired from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderExpired obj)


cdef class OrderTriggered(OrderEvent):

    @staticmethod
    cdef OrderTriggered from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderTriggered obj)


cdef class OrderPendingUpdate(OrderEvent):

    @staticmethod
    cdef OrderPendingUpdate from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderPendingUpdate obj)


cdef class OrderPendingCancel(OrderEvent):

    @staticmethod
    cdef OrderPendingCancel from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderPendingCancel obj)


cdef class OrderModifyRejected(OrderEvent):
    cdef readonly str reason
    """The reason for modify order rejection.\n\n:returns: `str`"""

    @staticmethod
    cdef OrderModifyRejected from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderModifyRejected obj)


cdef class OrderCancelRejected(OrderEvent):
    cdef readonly str reason
    """The reason for order cancel rejection.\n\n:returns: `str`"""

    @staticmethod
    cdef OrderCancelRejected from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderCancelRejected obj)


cdef class OrderUpdated(OrderEvent):
    cdef readonly Quantity quantity
    """The orders current quantity.\n\n:returns: `Quantity`"""
    cdef readonly Price price
    """The orders current price.\n\n:returns: `Price`"""
    cdef readonly Price trigger_price
    """The orders current trigger price.\n\n:returns: `Price` or ``None``"""

    @staticmethod
    cdef OrderUpdated from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderUpdated obj)


cdef class OrderFilled(OrderEvent):
    cdef readonly TradeId trade_id
    """The trade match ID (assigned by the venue).\n\n:returns: `TradeId`"""
    cdef readonly PositionId position_id
    """The position ID (assigned by the venue).\n\n:returns: `PositionId` or ``None``"""
    cdef readonly OrderSide order_side
    """The order side.\n\n:returns: `OrderSide`"""
    cdef readonly OrderType order_type
    """The order type.\n\n:returns: `OrderType`"""
    cdef readonly Quantity last_qty
    """The fill quantity.\n\n:returns: `Quantity`"""
    cdef readonly Price last_px
    """The fill price for this execution.\n\n:returns: `Price`"""
    cdef readonly Currency currency
    """The currency of the price.\n\n:returns: `Currency`"""
    cdef readonly Money commission
    """The commission generated from the fill.\n\n:returns: `Money`"""
    cdef readonly LiquiditySide liquidity_side
    """The liquidity side of the event {``MAKER``, ``TAKER``}.\n\n:returns: `LiquiditySide`"""
    cdef readonly dict info
    """The additional fill information.\n\n:returns: `dict[str, object]`"""

    @staticmethod
    cdef OrderFilled from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderFilled obj)
    cdef bint is_buy_c(self) except *
    cdef bint is_sell_c(self) except *
