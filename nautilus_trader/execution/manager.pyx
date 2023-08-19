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

from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport SENT
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.enums_c cimport ContingencyType
from nautilus_trader.model.enums_c cimport OrderStatus
from nautilus_trader.model.enums_c cimport TriggerType
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.events.position cimport PositionEvent
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class OrderManager:
    """
    Provides a generic order execution manager.

    Parameters
    ----------
    clock : Clock
        The clock for the order manager.
    logger : Logger
        The logger for the order manager.
    msgbus : MessageBus
        The message bus for the order manager.
    cache : Cache
        The cache for the order manager.
    component_name : str
        The component name for the order manager.

    """

    def __init__(
        self,
        Clock clock not None,
        Logger logger not None,
        MessageBus msgbus,
        Cache cache not None,
        str component_name not None,
    ):
        Condition.valid_string(component_name, "component_name")

        self._clock = clock
        self._log = LoggerAdapter(component_name=component_name, logger=logger)
        self._msgbus = msgbus
        self._cache = cache

    cpdef void cancel_order(self, Order order):
        Condition.not_none(order, "order")

        self._log.debug(f"Cancelling order {order}.")

        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderCanceled event = OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,  # Probably None
            account_id=order.account_id,  # Probably None
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self.send_exec_event(event)

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void handle_position_event(self, PositionEvent event):
        Condition.not_none(event, "event")
        # TBC

    cpdef void handle_order_rejected(self, OrderRejected rejected):
        Condition.not_none(rejected, "rejected")

        cdef Order order = self._cache.order(rejected.client_order_id)
        if order is None:
            self._log.error(
                "Cannot handle `OrderRejected`: "
                f"order for {repr(rejected.client_order_id)} not found. {rejected}",
                )
            return

        if order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self.handle_contingencies(order)

    cpdef void handle_order_canceled(self, OrderCanceled canceled):
        Condition.not_none(canceled, "canceled")

        cdef Order order = self._cache.order(canceled.client_order_id)
        if order is None:
            self._log.error(
                "Cannot handle `OrderCanceled`: "
                f"order for {repr(canceled.client_order_id)} not found. {canceled}",
                )
            return

        if order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self.handle_contingencies(order)

    cpdef void handle_order_expired(self, OrderExpired expired):
        Condition.not_none(expired, "expired")

        cdef Order order = self._cache.order(expired.client_order_id)
        if order is None:
            self._log.error(
                "Cannot handle `OrderExpired`: "
                f"order for {repr(expired.client_order_id)} not found. {expired}",
                )
            return

        if order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self.handle_contingencies(order)

    cpdef void handle_order_updated(self, OrderUpdated updated):
        Condition.not_none(updated, "updated")

        cdef Order order = self._cache.order(updated.client_order_id)
        if order is None:
            self._log.error(
                "Cannot handle `OrderUpdated`: "
                f"order for {repr(updated.client_order_id)} not found. {updated}",
                )
            return

        if order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self.handle_contingencies_update(order)


    cpdef void handle_order_filled(self, OrderFilled filled):
        Condition.not_none(filled, "filled")

        cdef Order order = self._cache.order(filled.client_order_id)
        if order is None:
            self._log.error(
                "Cannot handle `OrderFilled`: "
                f"order for {repr(filled.client_order_id)} not found. {filled}",
            )
            return

        cdef:
            PositionId position_id
            ClientId client_id
            ClientOrderId client_order_id
            Order child_order
            Order primary_order
            Order spawn_order
            Quantity parent_quantity
            Quantity parent_filled_qty
        if order.contingency_type == ContingencyType.OTO:
            Condition.not_empty(order.linked_order_ids, "order.linked_order_ids")

            position_id = self._cache.position_id(order.client_order_id)
            client_id = self._cache.client_id(order.client_order_id)

            if order.exec_spawn_id is not None:
                # Determine total quantities of execution spawn sequence
                parent_quantity = self._cache.exec_spawn_total_quantity(order.exec_spawn_id)
                parent_filled_qty = self._cache.exec_spawn_total_filled_qty(order.exec_spawn_id)
            else:
                parent_quantity = order.quantity
                parent_filled_qty = order.filled_qty

            for client_order_id in order.linked_order_ids:
                child_order = self._cache.order(client_order_id)
                assert child_order, f"Cannot find child order for {repr(client_order_id)}"
                if child_order.is_closed_c() or child_order.status == OrderStatus.RELEASED:
                    continue

                if child_order.position_id is None:
                    child_order.position_id = position_id

                if parent_filled_qty._mem.raw != child_order.leaves_qty._mem.raw:
                    self.update_order_quantity(child_order, parent_filled_qty)
                elif parent_quantity._mem.raw != child_order.quantity._mem.raw:
                    self.update_order_quantity(child_order, parent_quantity)
        elif order.contingency_type == ContingencyType.OCO:
            # Cancel all OCO orders
            for client_order_id in order.linked_order_ids:
                contingent_order = self._cache.order(client_order_id)
                assert contingent_order
                if contingent_order.is_closed_c():
                    continue
                if contingent_order.client_order_id != order.client_order_id:
                    self.cancel_order(contingent_order)
        elif order.contingency_type == ContingencyType.OUO:
            self.handle_contingencies(order)

    cpdef void handle_contingencies(self, Order order):
        Condition.not_none(order, "order")
        Condition.not_empty(order.linked_order_ids, "order.linked_order_ids")

        cdef:
            Quantity filled_qty
            Quantity leaves_qty
            bint is_spawn_active = False
        if order.exec_spawn_id is not None:
            # Determine total quantities of execution spawn sequence
            filled_qty = self._cache.exec_spawn_total_filled_qty(order.exec_spawn_id)
            leaves_qty = self._cache.exec_spawn_total_leaves_qty(order.exec_spawn_id, active_only=True)
            is_spawn_active = leaves_qty._mem.raw > 0
        else:
            filled_qty = order.filled_qty
            leaves_qty = order.leaves_qty

        cdef ClientOrderId client_order_id
        cdef Order contingent_order
        for client_order_id in order.linked_order_ids:
            contingent_order = self._cache.order(client_order_id)
            assert contingent_order
            if client_order_id == order.client_order_id:
                continue  # Already being handled
            if contingent_order.is_closed_c() or contingent_order.emulation_trigger == TriggerType.NO_TRIGGER:
                continue  # Already completed

            if order.contingency_type == ContingencyType.OTO:
                if order.is_closed_c() and filled_qty._mem.raw == 0 and (order.exec_spawn_id is None or not is_spawn_active):
                    self.cancel_order(contingent_order)
                elif filled_qty._mem.raw > 0 and filled_qty._mem.raw != contingent_order.quantity._mem.raw:
                    self.update_order_quantity(contingent_order, filled_qty)
            elif order.contingency_type == ContingencyType.OUO:
                if leaves_qty._mem.raw == 0 and order.exec_spawn_id is not None:
                    self.cancel_order(contingent_order)
                elif order.is_closed_c() and (order.exec_spawn_id is None or not is_spawn_active):
                    self.cancel_order(contingent_order)
                elif leaves_qty._mem.raw != contingent_order.leaves_qty._mem.raw:
                    self.update_order_quantity(contingent_order, leaves_qty)

    cpdef void handle_contingencies_update(self, Order order):
        Condition.not_none(order, "order")

        cdef:
            Quantity quantity
        if order.exec_spawn_id is not None:
            # Determine total quantity of execution spawn sequence
            quantity = self._cache.exec_spawn_total_quantity(order.exec_spawn_id, active_only=True)
        else:
            quantity = order.quantity

        if quantity._mem.raw == 0:
            return

        cdef ClientOrderId client_order_id
        cdef Order contingent_order
        for client_order_id in order.linked_order_ids:
            contingent_order = self._cache.order(client_order_id)
            assert contingent_order
            if client_order_id == order.client_order_id:
                continue  # Already being handled
            if contingent_order.is_closed_c() or contingent_order.emulation_trigger == TriggerType.NO_TRIGGER:
                self._commands_submit_order.pop(order.client_order_id, None)
                continue  # Already completed

            if order.contingency_type == ContingencyType.OTO:
                if quantity._mem.raw != contingent_order.quantity._mem.raw:
                    self.update_order_quantity(contingent_order, quantity)
            elif order.contingency_type == ContingencyType.OUO:
                if quantity._mem.raw != contingent_order.quantity._mem.raw:
                    self.update_order_quantity(contingent_order, quantity)

    cpdef void update_order_quantity(self, Order order, Quantity new_quantity):
        self._log.debug(f"Update contingency order {order.client_order_id!r} to {new_quantity}.")
        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        cdef OrderUpdated event = OrderUpdated(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=None,  # Not yet assigned by any venue
            account_id=order.account_id,  # Probably None
            quantity=new_quantity,
            price=None,
            trigger_price=None,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )
        order.apply(event)
        self._cache.update_order(order)

        self.send_risk_event(event)

# -- EGRESS ---------------------------------------------------------------------------------------

    cpdef void send_emulator_command(self, TradingCommand command):
        Condition.not_none(command, "command")

        if not self._log.is_bypassed:
            self._log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="OrderEmulator.execute", msg=command)

    cpdef void send_algo_command(self, TradingCommand command):
        Condition.not_none(command, "command")

        if not self._log.is_bypassed:
            self._log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint=f"{command.exec_algorithm_id}.execute", msg=command)

    cpdef void send_risk_command(self, TradingCommand command):
        Condition.not_none(command, "command")

        if not self._log.is_bypassed:
            self._log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="RiskEngine.execute", msg=command)

    cpdef void send_exec_command(self, TradingCommand command):
        Condition.not_none(command, "command")

        if not self._log.is_bypassed:
            self._log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="ExecEngine.execute", msg=command)

    cpdef void send_risk_event(self, OrderEvent event):
        Condition.not_none(event, "event")

        if not self._log.is_bypassed:
            self._log.info(f"{EVT}{SENT} {event}.")
        self._msgbus.send(endpoint="RiskEngine.process", msg=event)

    cpdef void send_exec_event(self, OrderEvent event):
        Condition.not_none(event, "event")

        if not self._log.is_bypassed:
            self._log.info(f"{EVT}{SENT} {event}.")
        self._msgbus.send(endpoint="ExecEngine.process", msg=event)
