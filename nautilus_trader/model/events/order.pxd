# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport uint64_t

from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport OrderAccepted_t
from nautilus_trader.core.rust.model cimport OrderDenied_t
from nautilus_trader.core.rust.model cimport OrderEmulated_t
from nautilus_trader.core.rust.model cimport OrderRejected_t
from nautilus_trader.core.rust.model cimport OrderReleased_t
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderSubmitted_t
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport TimeInForce
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class OrderEvent(Event):
    pass  # Abstract base class


cdef class OrderInitialized(OrderEvent):
    cdef TraderId _trader_id
    cdef StrategyId _strategy_id
    cdef InstrumentId _instrument_id
    cdef ClientOrderId _client_order_id
    cdef bint _reconciliation
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

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
    cdef readonly bint quote_quantity
    """If the order quantity is denominated in the quote currency.\n\n:returns: `bool`"""
    cdef readonly dict options
    """The order initialization options.\n\n:returns: `dict`"""
    cdef readonly TriggerType emulation_trigger
    """The order emulation trigger type.\n\n:returns: `TriggerType`"""
    cdef readonly InstrumentId trigger_instrument_id
    """The order emulation trigger instrument ID (will be `instrument_id` if ``None``).\n\n:returns: `InstrumentId` or ``None``"""
    cdef readonly ContingencyType contingency_type
    """The orders contingency type.\n\n:returns: `ContingencyType`"""
    cdef readonly OrderListId order_list_id
    """The order list ID associated with the order.\n\n:returns: `OrderListId` or ``None``"""
    cdef readonly list linked_order_ids
    """The orders linked client order ID(s).\n\n:returns: `list[ClientOrderId]` or ``None``"""
    cdef readonly ClientOrderId parent_order_id
    """The orders parent client order ID.\n\n:returns: `ClientOrderId` or ``None``"""
    cdef readonly ExecAlgorithmId exec_algorithm_id
    """The execution algorithm ID for the order.\n\n:returns: `ExecAlgorithmId` or ``None``"""
    cdef readonly dict exec_algorithm_params
    """The execution algorithm parameters for the order.\n\n:returns: `dict[str, Any]` or ``None``"""
    cdef readonly ClientOrderId exec_spawn_id
    """The execution algorithm spawning client order ID.\n\n:returns: `ClientOrderId` or ``None``"""
    cdef readonly list[str] tags
    """The order custom user tags.\n\n:returns: `list[str]` or ``None``"""

    @staticmethod
    cdef OrderInitialized from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderInitialized obj)


cdef class OrderDenied(OrderEvent):
    cdef OrderDenied_t _mem

    @staticmethod
    cdef OrderDenied from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderDenied obj)



cdef class OrderEmulated(OrderEvent):
    cdef OrderEmulated_t _mem

    @staticmethod
    cdef OrderEmulated from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderEmulated obj)


cdef class OrderReleased(OrderEvent):
    cdef OrderReleased_t _mem

    @staticmethod
    cdef OrderReleased from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderReleased obj)


cdef class OrderSubmitted(OrderEvent):
    cdef OrderSubmitted_t _mem

    @staticmethod
    cdef OrderSubmitted from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderSubmitted obj)


cdef class OrderAccepted(OrderEvent):
    cdef OrderAccepted_t _mem

    @staticmethod
    cdef OrderAccepted from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderAccepted obj)


cdef class OrderRejected(OrderEvent):
    cdef OrderRejected_t _mem

    @staticmethod
    cdef OrderRejected from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderRejected obj)


cdef class OrderCanceled(OrderEvent):
    cdef TraderId _trader_id
    cdef StrategyId _strategy_id
    cdef InstrumentId _instrument_id
    cdef ClientOrderId _client_order_id
    cdef VenueOrderId _venue_order_id
    cdef AccountId _account_id
    cdef bint _reconciliation
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

    @staticmethod
    cdef OrderCanceled from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderCanceled obj)


cdef class OrderExpired(OrderEvent):
    cdef TraderId _trader_id
    cdef StrategyId _strategy_id
    cdef InstrumentId _instrument_id
    cdef ClientOrderId _client_order_id
    cdef VenueOrderId _venue_order_id
    cdef AccountId _account_id
    cdef bint _reconciliation
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

    @staticmethod
    cdef OrderExpired from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderExpired obj)


cdef class OrderTriggered(OrderEvent):
    cdef TraderId _trader_id
    cdef StrategyId _strategy_id
    cdef InstrumentId _instrument_id
    cdef ClientOrderId _client_order_id
    cdef VenueOrderId _venue_order_id
    cdef AccountId _account_id
    cdef bint _reconciliation
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

    @staticmethod
    cdef OrderTriggered from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderTriggered obj)


cdef class OrderPendingUpdate(OrderEvent):
    cdef TraderId _trader_id
    cdef StrategyId _strategy_id
    cdef InstrumentId _instrument_id
    cdef ClientOrderId _client_order_id
    cdef VenueOrderId _venue_order_id
    cdef AccountId _account_id
    cdef bint _reconciliation
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

    @staticmethod
    cdef OrderPendingUpdate from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderPendingUpdate obj)


cdef class OrderPendingCancel(OrderEvent):
    cdef TraderId _trader_id
    cdef StrategyId _strategy_id
    cdef InstrumentId _instrument_id
    cdef ClientOrderId _client_order_id
    cdef VenueOrderId _venue_order_id
    cdef AccountId _account_id
    cdef bint _reconciliation
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

    @staticmethod
    cdef OrderPendingCancel from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderPendingCancel obj)


cdef class OrderModifyRejected(OrderEvent):
    cdef TraderId _trader_id
    cdef StrategyId _strategy_id
    cdef InstrumentId _instrument_id
    cdef ClientOrderId _client_order_id
    cdef VenueOrderId _venue_order_id
    cdef AccountId _account_id
    cdef str _reason
    cdef bint _reconciliation
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

    @staticmethod
    cdef OrderModifyRejected from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderModifyRejected obj)


cdef class OrderCancelRejected(OrderEvent):
    cdef TraderId _trader_id
    cdef StrategyId _strategy_id
    cdef InstrumentId _instrument_id
    cdef ClientOrderId _client_order_id
    cdef VenueOrderId _venue_order_id
    cdef AccountId _account_id
    cdef str _reason
    cdef bint _reconciliation
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

    @staticmethod
    cdef OrderCancelRejected from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(OrderCancelRejected obj)


cdef class OrderUpdated(OrderEvent):
    cdef TraderId _trader_id
    cdef StrategyId _strategy_id
    cdef InstrumentId _instrument_id
    cdef ClientOrderId _client_order_id
    cdef VenueOrderId _venue_order_id
    cdef AccountId _account_id
    cdef bint _reconciliation
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

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
    cdef TraderId _trader_id
    cdef StrategyId _strategy_id
    cdef InstrumentId _instrument_id
    cdef ClientOrderId _client_order_id
    cdef VenueOrderId _venue_order_id
    cdef AccountId _account_id
    cdef bint _reconciliation
    cdef UUID4 _event_id
    cdef uint64_t _ts_event
    cdef uint64_t _ts_init

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
    cdef bint is_buy_c(self)
    cdef bint is_sell_c(self)
