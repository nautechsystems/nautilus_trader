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

from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport CMD
from nautilus_trader.common.component cimport EVT
from nautilus_trader.common.component cimport SENT
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport LogColor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport is_logging_initialized
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport OrderStatus
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
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


cdef class OrderManager:
    """
    Provides a generic order execution manager.

    Parameters
    ----------
    clock : Clock
        The clock for the order manager.
    msgbus : MessageBus
        The message bus for the order manager.
    cache : Cache
        The cache for the order manager.
    component_name : str
        The component name for the order manager.
    active_local : str
        If the manager is for active local orders.
    submit_order_handler : Callable[[SubmitOrder], None], optional
        The handler to call when submitting orders.
    cancel_order_handler : Callable[[Order], None], optional
        The handler to call when canceling orders.
    modify_order_handler : Callable[[Order, Quantity], None], optional
        The handler to call when modifying orders (limited to modifying quantity).
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    Raises
    ------
    TypeError
        If `submit_order_handler` is not ``None`` and not of type `Callable`.
    TypeError
        If `cancel_order_handler` is not ``None`` and not of type `Callable`.
    TypeError
        If `modify_order_handler` is not ``None`` and not of type `Callable`.
    """

    def __init__(
        self,
        Clock clock not None,
        MessageBus msgbus,
        Cache cache not None,
        str component_name not None,
        bint active_local,
        submit_order_handler: Callable[[SubmitOrder], None] = None,
        cancel_order_handler: Callable[[Order], None] = None,
        modify_order_handler: Callable[[Order, Quantity], None] = None,
        bint debug = False,
        bint log_events = True,
        bint log_commands = True,
    ):
        Condition.valid_string(component_name, "component_name")
        Condition.callable_or_none(submit_order_handler, "submit_order_handler")
        Condition.callable_or_none(cancel_order_handler, "cancel_order_handler")
        Condition.callable_or_none(modify_order_handler, "modify_order_handler")

        self._clock = clock
        self._log = Logger(name=component_name)
        self._msgbus = msgbus
        self._cache = cache

        self.active_local = active_local
        self.debug = debug
        self.log_events = log_events
        self.log_commands = log_commands
        self._submit_order_handler = submit_order_handler
        self._cancel_order_handler = cancel_order_handler
        self._modify_order_handler = modify_order_handler

        self._submit_order_commands: dict[ClientOrderId, SubmitOrder] = {}

    cpdef dict get_submit_order_commands(self):
        """
        Return the managers cached submit order commands.

        Returns
        -------
        dict[ClientOrderId, SubmitOrder]

        """
        return self._submit_order_commands.copy()

    cpdef void cache_submit_order_command(self, SubmitOrder command):
        """
        Cache the given submit order `command` with the manager.

        Parameters
        ----------
        command : SubmitOrder
            The submit order command to cache.

        """
        Condition.not_none(command, "command")

        self._submit_order_commands[command.order.client_order_id] = command

    cpdef SubmitOrder pop_submit_order_command(self, ClientOrderId client_order_id):
        """
        Pop the submit order command for the given `client_order_id` out of the managers
        cache (if found).

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID for the command to pop.

        Returns
        -------
        SubmitOrder or ``None``

        """
        Condition.not_none(client_order_id, "client_order_id")

        return self._submit_order_commands.pop(client_order_id, None)

    cpdef void reset(self):
        """
        Reset the manager, clearing all stateful values.
        """
        self._submit_order_commands.clear()

    cpdef void cancel_order(self, Order order):
        """
        Cancel the given `order` with the manager.

        Parameters
        ----------
        order : Order
            The order to cancel.

        """
        Condition.not_none(order, "order")

        if self._cache.is_order_pending_cancel_local(order.client_order_id):
            return  # Already pending cancel locally

        if order.is_closed_c():
            self._log.warning("Cannot cancel order: already closed")
            return

        if self.debug:
            self._log.info(f"Canceling order {order}", LogColor.MAGENTA)

        self._submit_order_commands.pop(order.client_order_id, None)

        if self._cancel_order_handler is not None:
            self._cancel_order_handler(order)

    cpdef void modify_order_quantity(self, Order order, Quantity new_quantity):
        """
        Modify the given `order` with the manager.

        Parameters
        ----------
        order : Order
            The order to modify.

        """
        Condition.not_none(order, "order")
        Condition.not_none(new_quantity, "new_quantity")

        if self._modify_order_handler is not None:
            self._modify_order_handler(order, new_quantity)

    cpdef void create_new_submit_order(
        self,
        Order order,
        PositionId position_id = None,
        ClientId client_id = None,
    ):
        """
        Create a new submit order command for the given `order`.

        Parameters
        ----------
        order : Order
            The order for the command.
        position_id : PositionId, optional
            The position ID for the command.
        client_id : ClientId, optional
            The client ID for the command.

        """
        Condition.not_none(order, "order")

        if self.debug:
            self._log.info(
                f"Creating new `SubmitOrder` command for {order}, {position_id=}, {client_id=}",
                LogColor.MAGENTA,
            )

        cdef SubmitOrder submit = SubmitOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            order=order,
            position_id=position_id,
            client_id=client_id,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        if order.emulation_trigger == TriggerType.NO_TRIGGER:
            # Cache command
            self.cache_submit_order_command(submit)

            if order.exec_algorithm_id is not None:
                self.send_algo_command(submit, order.exec_algorithm_id)
            else:
                self.send_risk_command(submit)
        else:
            self._submit_order_handler(submit)

    cpdef bint should_manage_order(self, Order order):
        """
        Check if the given order should be managed.

        Parameters
        ----------
        order : Order
            The order to check.

        Returns
        -------
        bool
            True if the order should be managed, else False.

        """
        Condition.not_none(order, "order")

        if self.active_local:
            return order.is_active_local_c()
        else:
            return not order.is_active_local_c()

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void handle_event(self, Event event):
        """
        Handle the given `event`.

        If a handler for the given event is not implemented then this will simply be a no-op.

        Parameters
        ----------
        event : Event
            The event to handle

        """
        if isinstance(event, OrderRejected):
            self.handle_order_rejected(event)
        elif isinstance(event, OrderCanceled):
            self.handle_order_canceled(event)
        elif isinstance(event, OrderExpired):
            self.handle_order_expired(event)
        elif isinstance(event, OrderUpdated):
            self.handle_order_updated(event)
        elif isinstance(event, OrderFilled):
            self.handle_order_filled(event)
        elif isinstance(event, PositionEvent):
            self.handle_position_event(event)

    cpdef void handle_order_rejected(self, OrderRejected rejected):
        Condition.not_none(rejected, "rejected")

        cdef Order order = self._cache.order(rejected.client_order_id)
        if order is None:
            self._log.error(  # pragma: no cover (design-time error)
                "Cannot handle `OrderRejected`: "
                f"order for {repr(rejected.client_order_id)} not found, {rejected}",
                )
            return

        if order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self.handle_contingencies(order)

    cpdef void handle_order_canceled(self, OrderCanceled canceled):
        Condition.not_none(canceled, "canceled")

        cdef Order order = self._cache.order(canceled.client_order_id)
        if order is None:
            self._log.error(  # pragma: no cover (design-time error)
                "Cannot handle `OrderCanceled`: "
                f"order for {repr(canceled.client_order_id)} not found, {canceled}",
                )
            return

        if order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self.handle_contingencies(order)

    cpdef void handle_order_expired(self, OrderExpired expired):
        Condition.not_none(expired, "expired")

        cdef Order order = self._cache.order(expired.client_order_id)
        if order is None:
            self._log.error(  # pragma: no cover (design-time error)
                "Cannot handle `OrderExpired`: "
                f"order for {repr(expired.client_order_id)} not found, {expired}",
                )
            return

        if order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self.handle_contingencies(order)

    cpdef void handle_order_updated(self, OrderUpdated updated):
        Condition.not_none(updated, "updated")

        cdef Order order = self._cache.order(updated.client_order_id)
        if order is None:
            self._log.error(  # pragma: no cover (design-time error)
                "Cannot handle `OrderUpdated`: "
                f"order for {repr(updated.client_order_id)} not found, {updated}",
                )
            return

        if order.contingency_type != ContingencyType.NO_CONTINGENCY:
            self.handle_contingencies_update(order)

    cpdef void handle_order_filled(self, OrderFilled filled):
        Condition.not_none(filled, "filled")

        if self.debug:
            self._log.info(f"Handling fill for {filled.client_order_id}", LogColor.MAGENTA)

        cdef Order order = self._cache.order(filled.client_order_id)
        if order is None:  # pragma: no cover (design-time error)
            self._log.error(
                "Cannot handle `OrderFilled`: "
                f"order for {repr(filled.client_order_id)} not found, {filled}",
            )
            return

        cdef:
            PositionId position_id
            ClientId client_id
            ClientOrderId client_order_id
            Order child_order
            Order primary_order
            Order spawn_order
            Quantity parent_filled_qty
        if order.contingency_type == ContingencyType.OTO:
            Condition.not_empty(order.linked_order_ids, "order.linked_order_ids")

            position_id = self._cache.position_id(order.client_order_id)
            client_id = self._cache.client_id(order.client_order_id)

            if order.exec_spawn_id is not None:
                # Determine total filled of execution spawn sequence
                parent_filled_qty = self._cache.exec_spawn_total_filled_qty(order.exec_spawn_id)
            else:
                parent_filled_qty = order.filled_qty

            for client_order_id in order.linked_order_ids:
                child_order = self._cache.order(client_order_id)
                if child_order is None:
                    raise RuntimeError(f"Cannot find OTO child order for {repr(client_order_id)}")  # pragma: no cover

                if not self.should_manage_order(child_order):
                    continue  # Not being managed

                if self.debug:
                    self._log.info(f"Processing OTO child order {child_order}", LogColor.MAGENTA)
                    self._log.info(f"{parent_filled_qty=}", LogColor.MAGENTA)

                if child_order.position_id is None:
                    child_order.position_id = position_id

                if parent_filled_qty._mem.raw != child_order.leaves_qty._mem.raw:
                    self.modify_order_quantity(child_order, parent_filled_qty)

                if self._submit_order_handler is None:
                    return  # No handler to submit

                if not child_order.client_order_id in self._submit_order_commands:
                    self.create_new_submit_order(
                        order=child_order,
                        position_id=position_id,
                        client_id=client_id,
                    )
        elif order.contingency_type == ContingencyType.OCO:
            # Cancel all OCO orders
            for client_order_id in order.linked_order_ids:
                contingent_order = self._cache.order(client_order_id)
                if contingent_order is None:
                    raise RuntimeError(f"Cannot find OCO contingent order for {repr(client_order_id)}")  # pragma: no cover

                if self.debug:
                    self._log.info(f"Processing OCO contingent order {contingent_order}", LogColor.MAGENTA)

                if not self.should_manage_order(contingent_order):
                    continue  # Not being managed
                if contingent_order.is_closed_c():
                    continue  # Already completed
                if contingent_order.client_order_id != order.client_order_id:
                    self.cancel_order(contingent_order)
        elif order.contingency_type == ContingencyType.OUO:
            self.handle_contingencies(order)

    cpdef void handle_contingencies(self, Order order):
        Condition.not_none(order, "order")
        Condition.not_empty(order.linked_order_ids, "order.linked_order_ids")

        if self.debug:
            self._log.info(
                f"Handling contingencies for {order.client_order_id}", LogColor.MAGENTA,
            )

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
            if contingent_order is None:
                raise RuntimeError(f"Cannot find contingent order for {repr(client_order_id)}")  # pragma: no cover
            if not self.should_manage_order(contingent_order):
                continue  # Not being managed
            if client_order_id == order.client_order_id:
                continue  # Already being handled
            if contingent_order.is_closed_c():
                self._submit_order_commands.pop(order.client_order_id, None)
                continue  # Already completed

            if order.contingency_type == ContingencyType.OTO:
                if self.debug:
                    self._log.info(f"Processing OTO child order {contingent_order}", LogColor.MAGENTA)
                    self._log.info(f"{filled_qty=}, {contingent_order.quantity=}", LogColor.YELLOW)
                if order.is_closed_c() and filled_qty._mem.raw == 0 and (order.exec_spawn_id is None or not is_spawn_active):
                    self.cancel_order(contingent_order)
                elif filled_qty._mem.raw > 0 and filled_qty._mem.raw != contingent_order.quantity._mem.raw:
                    self.modify_order_quantity(contingent_order, filled_qty)
            elif order.contingency_type == ContingencyType.OCO:
                if self.debug:
                    self._log.info(f"Processing OCO contingent order {client_order_id}", LogColor.MAGENTA)
                if order.is_closed_c() and (order.exec_spawn_id is None or not is_spawn_active):
                    self.cancel_order(contingent_order)
            elif order.contingency_type == ContingencyType.OUO:
                if self.debug:
                    self._log.info(f"Processing OUO contingent order {client_order_id}, {leaves_qty=}, {contingent_order.leaves_qty=}", LogColor.MAGENTA)
                if leaves_qty._mem.raw == 0 and order.exec_spawn_id is not None:
                    self.cancel_order(contingent_order)
                elif order.is_closed_c() and (order.exec_spawn_id is None or not is_spawn_active):
                    self.cancel_order(contingent_order)
                elif leaves_qty._mem.raw != contingent_order.leaves_qty._mem.raw:
                    self.modify_order_quantity(contingent_order, leaves_qty)

    cpdef void handle_contingencies_update(self, Order order):
        Condition.not_none(order, "order")

        if self.debug:
            self._log.info(
                f"Handling contingencies update for {order.client_order_id}", LogColor.MAGENTA,
            )

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
            if contingent_order is None:
                raise RuntimeError(f"Cannot find OCO contingent order for {repr(client_order_id)}")  # pragma: no cover

            if not self.should_manage_order(contingent_order):
                continue  # Not being managed
            if client_order_id == order.client_order_id:
                continue  # Already being handled  # pragma: no cover
            if contingent_order.is_closed_c():
                continue  # Already completed  # pragma: no cover

            if order.contingency_type == ContingencyType.OTO:
                if quantity._mem.raw != contingent_order.quantity._mem.raw:
                    self.modify_order_quantity(contingent_order, quantity)
            elif order.contingency_type == ContingencyType.OUO:
                if quantity._mem.raw != contingent_order.quantity._mem.raw:
                    self.modify_order_quantity(contingent_order, quantity)

    cpdef void handle_position_event(self, PositionEvent event):
        Condition.not_none(event, "event")
        # TBC

# -- EGRESS ---------------------------------------------------------------------------------------

    cpdef void send_emulator_command(self, TradingCommand command):
        Condition.not_none(command, "command")

        if self.log_commands and is_logging_initialized():
            self._log.info(f"{CMD}{SENT} {command}")  # pragma: no cover  (no logging in tests)
        self._msgbus.send(endpoint="OrderEmulator.execute", msg=command)

    cpdef void send_algo_command(self, TradingCommand command, ExecAlgorithmId exec_algorithm_id):
        Condition.not_none(command, "command")
        Condition.not_none(exec_algorithm_id, "exec_algorithm_id")

        if self.log_commands and is_logging_initialized():
            self._log.info(f"{CMD}{SENT} {command}")  # pragma: no cover  (no logging in tests)
        self._msgbus.send(endpoint=f"{exec_algorithm_id}.execute", msg=command)

    cpdef void send_risk_command(self, TradingCommand command):
        Condition.not_none(command, "command")

        if self.log_commands and is_logging_initialized():
            self._log.info(f"{CMD}{SENT} {command}")  # pragma: no cover  (no logging in tests)
        self._msgbus.send(endpoint="RiskEngine.execute", msg=command)

    cpdef void send_exec_command(self, TradingCommand command):
        Condition.not_none(command, "command")

        if self.log_commands and is_logging_initialized():
            self._log.info(f"{CMD}{SENT} {command}")  # pragma: no cover  (no logging in tests)
        self._msgbus.send(endpoint="ExecEngine.execute", msg=command)

    cpdef void send_risk_event(self, OrderEvent event):
        Condition.not_none(event, "event")

        if self.log_events and is_logging_initialized():
            self._log.info(f"{EVT}{SENT} {event}")  # pragma: no cover  (no logging in tests)
        self._msgbus.send(endpoint="RiskEngine.process", msg=event)

    cpdef void send_exec_event(self, OrderEvent event):
        Condition.not_none(event, "event")

        if self.log_events and is_logging_initialized():
            self._log.info(f"{EVT}{SENT} {event}")  # pragma: no cover (no logging in tests)
        self._msgbus.send(endpoint="ExecEngine.process", msg=event)
