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

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.rust.model cimport TimeInForce
from nautilus_trader.execution.manager cimport OrderManager
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderCancelRejected
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.events.order cimport OrderEmulated
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderModifyRejected
from nautilus_trader.model.events.order cimport OrderPendingCancel
from nautilus_trader.model.events.order cimport OrderPendingUpdate
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderReleased
from nautilus_trader.model.events.order cimport OrderSubmitted
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.events.position cimport PositionChanged
from nautilus_trader.model.events.position cimport PositionClosed
from nautilus_trader.model.events.position cimport PositionEvent
from nautilus_trader.model.events.position cimport PositionOpened
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.position cimport Position
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class Strategy(Actor):
    cdef OrderManager _manager

    cdef readonly OrderFactory order_factory
    """The order factory for the strategy.\n\n:returns: `OrderFactory`"""
    cdef readonly str order_id_tag
    """The order ID tag for the strategy.\n\n:returns: `str`"""
    cdef readonly bint use_uuid_client_order_ids
    """If UUID4's should be used for client order ID values.\n\n:returns: `bool`"""
    cdef readonly OmsType oms_type
    """The order management system for the strategy.\n\n:returns: `OmsType`"""
    cdef readonly list external_order_claims
    """The external order claims instrument IDs for the strategy.\n\n:returns: `list[InstrumentId]`"""
    cdef readonly bint manage_contingent_orders
    """If contingent orders should be managed automatically by the strategy.\n\n:returns: `bool`"""
    cdef readonly bint manage_gtd_expiry
    """If all order GTD time in force expirations should be managed automatically by the strategy.\n\n:returns: `bool`"""

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register(
        self,
        TraderId trader_id,
        PortfolioFacade portfolio,
        MessageBus msgbus,
        CacheFacade cache,
        Clock clock,
    )
    cpdef void change_id(self, StrategyId strategy_id)
    cpdef void change_order_id_tag(self, str order_id_tag)

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void on_order_event(self, OrderEvent event)
    cpdef void on_order_initialized(self, OrderInitialized event)
    cpdef void on_order_denied(self, OrderDenied event)
    cpdef void on_order_emulated(self, OrderEmulated event)
    cpdef void on_order_released(self, OrderReleased event)
    cpdef void on_order_submitted(self, OrderSubmitted event)
    cpdef void on_order_rejected(self, OrderRejected event)
    cpdef void on_order_accepted(self, OrderAccepted event)
    cpdef void on_order_canceled(self, OrderCanceled event)
    cpdef void on_order_expired(self, OrderExpired event)
    cpdef void on_order_triggered(self, OrderTriggered event)
    cpdef void on_order_pending_update(self, OrderPendingUpdate event)
    cpdef void on_order_pending_cancel(self, OrderPendingCancel event)
    cpdef void on_order_modify_rejected(self, OrderModifyRejected event)
    cpdef void on_order_cancel_rejected(self, OrderCancelRejected event)
    cpdef void on_order_updated(self, OrderUpdated event)
    cpdef void on_order_filled(self, OrderFilled event)
    cpdef void on_position_event(self, PositionEvent event)
    cpdef void on_position_opened(self, PositionOpened event)
    cpdef void on_position_changed(self, PositionChanged event)
    cpdef void on_position_closed(self, PositionClosed event)

# -- TRADING COMMANDS -----------------------------------------------------------------------------

    cpdef void submit_order(
        self,
        Order order,
        PositionId position_id=*,
        ClientId client_id=*, dict[str, object] params=*,
    )
    cpdef void submit_order_list(
        self,
        OrderList order_list,
        PositionId position_id=*,
        ClientId client_id=*, dict[str, object] params=*,
    )
    cpdef void modify_order(
        self,
        Order order,
        Quantity quantity=*,
        Price price=*,
        Price trigger_price=*,
        ClientId client_id=*, dict[str, object] params=*,
    )
    cpdef void cancel_order(self, Order order, ClientId client_id=*, dict[str, object] params=*)
    cpdef void cancel_orders(self, list orders, ClientId client_id=*, dict[str, object] params=*)
    cpdef void cancel_all_orders(self, InstrumentId instrument_id, OrderSide order_side=*, ClientId client_id=*, dict[str, object] params=*)
    cpdef void close_position(self, Position position, ClientId client_id=*, list[str] tags=*, TimeInForce time_in_force=*, bint reduce_only=*, dict[str, object] params=*)
    cpdef void close_all_positions(self, InstrumentId instrument_id, PositionSide position_side=*, ClientId client_id=*, list[str] tags=*, TimeInForce time_in_force=*, bint reduce_only=*, dict[str, object] params=*)
    cpdef void query_order(self, Order order, ClientId client_id=*, dict[str, object] params=*)
    cdef ModifyOrder _create_modify_order(
        self,
        Order order,
        Quantity quantity=*,
        Price price=*,
        Price trigger_price=*,
        ClientId client_id=*,
        dict[str, object] params=*,
    )
    cdef CancelOrder _create_cancel_order(self, Order order, ClientId client_id=*, dict[str, object] params=*)

    cpdef void cancel_gtd_expiry(self, Order order)
    cdef bint _has_gtd_expiry_timer(self, ClientOrderId client_order_id)
    cdef str _get_gtd_expiry_timer_name(self, ClientOrderId client_order_id)
    cdef void _set_gtd_expiry(self, Order order)
    cpdef void _expire_gtd_order(self, TimeEvent event)

# -- EVENTS ---------------------------------------------------------------------------------------

    cdef OrderDenied _generate_order_denied(self, Order order, str reason)
    cdef OrderPendingUpdate _generate_order_pending_update(self, Order order)
    cdef OrderPendingCancel _generate_order_pending_cancel(self, Order order)
    cdef void _deny_order(self, Order order, str reason)
    cdef void _deny_order_list(self, OrderList order_list, str reason)
