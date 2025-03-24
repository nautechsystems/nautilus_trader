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

from typing import Any

from nautilus_trader.execution.config import ExecAlgorithmConfig
from nautilus_trader.execution.config import ImportableExecAlgorithmConfig

from cpython.datetime cimport datetime
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.component cimport CMD
from nautilus_trader.common.component cimport RECV
from nautilus_trader.common.component cimport SENT
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport LogColor
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport is_logging_initialized
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.model cimport ContingencyType
from nautilus_trader.core.rust.model cimport OrderStatus
from nautilus_trader.core.rust.model cimport TimeInForce
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
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
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport LIMIT_ORDER_TYPES
from nautilus_trader.model.orders.base cimport STOP_ORDER_TYPES
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.market_to_limit cimport MarketToLimitOrder
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class ExecAlgorithm(Actor):
    """
    The base class for all execution algorithms.

    This class allows traders to implement their own customized execution algorithms.

    Parameters
    ----------
    config : ExecAlgorithmConfig, optional
        The execution algorithm configuration.

    Raises
    ------
    TypeError
        If `config` is not of type `ExecAlgorithmConfig`.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, config: ExecAlgorithmConfig | None = None):
        if config is None:
            config = ExecAlgorithmConfig()
        Condition.type(config, ExecAlgorithmConfig, "config")

        super().__init__()
        # Assign Execution Algorithm ID after base class initialized
        if isinstance(config.exec_algorithm_id, str):
            self.id = ExecAlgorithmId(config.exec_algorithm_id)
        else:
            self.id = config.exec_algorithm_id or ExecAlgorithmId(type(self).__name__)

        # Configuration
        self._log_events = config.log_events
        self._log_commands = config.log_commands
        self.config = config

        self._exec_spawn_ids: dict[ClientOrderId, int] = {}
        self._subscribed_strategies: set[StrategyId] = set()

        # Public components
        self.portfolio = None  # Initialized when registered

    def to_importable_config(self) -> ImportableExecAlgorithmConfig:
        """
        Returns an importable configuration for this execution algorithm.

        Returns
        -------
        ImportableExecAlgorithmConfig

        """
        return ImportableExecAlgorithmConfig(
            exec_algorithm_path=self.fully_qualified_name(),
            config_path=self.config.fully_qualified_name(),
            config=self.config.dict(),
        )

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register(
        self,
        TraderId trader_id,
        PortfolioFacade portfolio,
        MessageBus msgbus,
        CacheFacade cache,
        Clock clock,
    ):
        """
        Register the execution algorithm with a trader.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID for the execution algorithm.
        portfolio : PortfolioFacade
            The read-only portfolio for the execution algorithm.
        msgbus : MessageBus
            The message bus for the execution algorithm.
        cache : CacheFacade
            The read-only cache for the execution algorithm.
        clock : Clock
            The clock for the execution algorithm.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(trader_id, "trader_id")
        Condition.not_none(portfolio, "portfolio")
        Condition.not_none(msgbus, "msgbus")
        Condition.not_none(cache, "cache")
        Condition.not_none(clock, "clock")

        self.register_base(
            portfolio=portfolio,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Register endpoints
        self._msgbus.register(endpoint=f"{self.id}.execute", handler=self.execute)

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _reset(self):
        self._exec_spawn_ids.clear()
        self._subscribed_strategies.clear()

        self.on_reset()

# -- INTERNAL -------------------------------------------------------------------------------------

    cdef ClientOrderId _spawn_client_order_id(self, Order primary):
        cdef int spawn_sequence = self._exec_spawn_ids.get(primary.client_order_id, 0)
        spawn_sequence += 1
        self._exec_spawn_ids[primary.client_order_id] = spawn_sequence

        return ClientOrderId(f"{primary.client_order_id.to_str()}-E{spawn_sequence}")

    cdef void _reduce_primary_order(self, Order primary, Quantity spawn_qty):
        Condition.is_true(primary.quantity >= spawn_qty, "Spawn order quantity was greater than or equal to primary order")

        cdef Quantity new_qty = Quantity.from_raw_c(
            primary.quantity._mem.raw - spawn_qty._mem.raw,
            primary.quantity._mem.precision,
        )

        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()

        cdef OrderUpdated updated = OrderUpdated(
            trader_id=primary.trader_id,
            strategy_id=primary.strategy_id,
            instrument_id=primary.instrument_id,
            client_order_id=primary.client_order_id,
            venue_order_id=primary.venue_order_id,
            account_id=primary.account_id,
            quantity=new_qty,
            price=None,
            trigger_price=None,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )

        primary.apply(updated)
        self.cache.update_order(primary)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void execute(self, TradingCommand command):
        """
        Handle the given trading command by processing it with the execution algorithm.

        Parameters
        ----------
        command : SubmitOrder
            The command to handle.

        Raises
        ------
        ValueError
            If `command.exec_algorithm_id` is not equal to `self.id`.

        """
        Condition.not_none(command, "command")

        if self._log_commands:
            self._log.debug(f"{RECV}{CMD} {command}", LogColor.MAGENTA)

        if self._fsm.state != ComponentState.RUNNING:
            return

        if isinstance(command, SubmitOrder):
            self._handle_submit_order(command)
        elif isinstance(command, SubmitOrderList):
            self._handle_submit_order_list(command)
        elif isinstance(command, CancelOrder):
            self._handle_cancel_order(command)
        else:
            self._log.error(f"Cannot handle command: unrecognized {command}")

        if command.strategy_id in self._subscribed_strategies:
            return  # Already subscribed

        self._log.info(f"Subscribing to {command.strategy_id} order events", LogColor.BLUE)
        self._msgbus.subscribe(topic=f"events.order.{command.strategy_id.to_str()}", handler=self._handle_event)
        self._msgbus.subscribe(topic=f"events.position.{command.strategy_id.to_str()}", handler=self._handle_event)
        self._subscribed_strategies.add(command.strategy_id)

    cdef void _handle_submit_order(self, SubmitOrder command):
        Condition.equal(command.exec_algorithm_id, self.id, "command.exec_algorithm_id", "self.id")
        try:
            self.on_order(command.order)
        except Exception as e:  # pragma: no cover
            self.log.exception(f"Error on handling {repr(command.order)}", e)
            raise

    cdef void _handle_submit_order_list(self, SubmitOrderList command):
        Condition.equal(command.exec_algorithm_id, self.id, "command.exec_algorithm_id", "self.id")
        cdef Order order
        for order in command.order_list.orders:
            if order.exec_algorithm_id is not None:
                Condition.equal(order.exec_algorithm_id, self.id, "order.exec_algorithm_id", "self.id")
        try:
            self.on_order_list(command.order_list)
        except Exception as e:  # pragma: no cover
            self.log.exception(f"Error on handling {repr(command.order_list)}", e)
            raise

    cdef void _handle_cancel_order(self, CancelOrder command):
        cdef Order order = self.cache.order(command.client_order_id)
        if order is None:  # pragma: no cover (design-time error)
            self._log.error(
                f"Cannot cancel order: {repr(command.client_order_id)} not found",
            )
            return

        if self.cache.is_order_pending_cancel_local(command.client_order_id):
            return  # Already pending cancel locally

        if order.is_closed_c():
            self._log.warning(f"Order already canceled for {command}")
            return

        # Generate event
        cdef OrderCanceled event = self._generate_order_canceled(order)

        try:
            order.apply(event)
        except InvalidStateTrigger as e:  # pragma: no cover
            self._log.warning(f"InvalidStateTrigger: {e}, did not apply {event}")
            return

        self.cache.update_order(order)

        # Publish canceled event
        self._msgbus.publish_c(
            topic=f"events.order.{order.strategy_id.to_str()}",
            msg=event,
        )

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event):
        cdef Order order

        if isinstance(event, OrderEvent):
            order = self.cache.order(event.client_order_id)
            if order is None:
                return
            if order.exec_algorithm_id is None or order.exec_algorithm_id != self.id:
                return  # Not for this algorithm

        if self._fsm.state != ComponentState.RUNNING:
            return

        try:
            # Send to specific event handler
            if isinstance(event, OrderInitialized):
                self.on_order_initialized(event)
                self.on_order_event(event)
            elif isinstance(event, OrderDenied):
                self.on_order_denied(event)
                self.on_order_event(event)
            elif isinstance(event, OrderEmulated):
                self.on_order_emulated(event)
                self.on_order_event(event)
            elif isinstance(event, OrderReleased):
                self.on_order_released(event)
                self.on_order_event(event)
            elif isinstance(event, OrderSubmitted):
                self.on_order_submitted(event)
                self.on_order_event(event)
            elif isinstance(event, OrderRejected):
                self.on_order_rejected(event)
                self.on_order_event(event)
            elif isinstance(event, OrderAccepted):
                self.on_order_accepted(event)
                self.on_order_event(event)
            elif isinstance(event, OrderCanceled):
                self.on_order_canceled(event)
                self.on_order_event(event)
            elif isinstance(event, OrderExpired):
                self.on_order_expired(event)
                self.on_order_event(event)
            elif isinstance(event, OrderTriggered):
                self.on_order_triggered(event)
                self.on_order_event(event)
            elif isinstance(event, OrderPendingUpdate):
                self.on_order_pending_update(event)
                self.on_order_event(event)
            elif isinstance(event, OrderPendingCancel):
                self.on_order_pending_cancel(event)
                self.on_order_event(event)
            elif isinstance(event, OrderModifyRejected):
                self.on_order_modify_rejected(event)
                self.on_order_event(event)
            elif isinstance(event, OrderCancelRejected):
                self.on_order_cancel_rejected(event)
                self.on_order_event(event)
            elif isinstance(event, OrderUpdated):
                self.on_order_updated(event)
                self.on_order_event(event)
            elif isinstance(event, OrderFilled):
                self.on_order_filled(event)
                self.on_order_event(event)
            elif isinstance(event, PositionOpened):
                self.on_position_opened(event)
                self.on_position_event(event)
            elif isinstance(event, PositionChanged):
                self.on_position_changed(event)
                self.on_position_event(event)
            elif isinstance(event, PositionClosed):
                self.on_position_closed(event)
                self.on_position_event(event)

            # Always send to general event handler
            self.on_event(event)
        except Exception as e:  # pragma: no cover
            self.log.exception(f"Error on handling {repr(event)}", e)
            raise

    cpdef void on_order(self, Order order):
        """
        Actions to be performed when running and receives an order.

        Parameters
        ----------
        order : Order
            The order to be handled.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_list(self, OrderList order_list):
        """
        Actions to be performed when running and receives an order list.

        Parameters
        ----------
        order_list : OrderList
            The order list to be handled.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_event(self, OrderEvent event):
        """
        Actions to be performed when running and receives an order event.

        Parameters
        ----------
        event : OrderEvent
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_initialized(self, OrderInitialized event):
        """
        Actions to be performed when running and receives an order initialized event.

        Parameters
        ----------
        event : OrderInitialized
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_denied(self, OrderDenied event):
        """
        Actions to be performed when running and receives an order denied event.

        Parameters
        ----------
        event : OrderDenied
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_emulated(self, OrderEmulated event):
        """
        Actions to be performed when running and receives an order initialized event.

        Parameters
        ----------
        event : OrderEmulated
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_released(self, OrderReleased event):
        """
        Actions to be performed when running and receives an order released event.

        Parameters
        ----------
        event : OrderReleased
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_submitted(self, OrderSubmitted event):
        """
        Actions to be performed when running and receives an order submitted event.

        Parameters
        ----------
        event : OrderSubmitted
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_rejected(self, OrderRejected event):
        """
        Actions to be performed when running and receives an order rejected event.

        Parameters
        ----------
        event : OrderRejected
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_accepted(self, OrderAccepted event):
        """
        Actions to be performed when running and receives an order accepted event.

        Parameters
        ----------
        event : OrderAccepted
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_canceled(self, OrderCanceled event):
        """
        Actions to be performed when running and receives an order canceled event.

        Parameters
        ----------
        event : OrderCanceled
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_expired(self, OrderExpired event):
        """
        Actions to be performed when running and receives an order expired event.

        Parameters
        ----------
        event : OrderExpired
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_triggered(self, OrderTriggered event):
        """
        Actions to be performed when running and receives an order triggered event.

        Parameters
        ----------
        event : OrderTriggered
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_pending_update(self, OrderPendingUpdate event):
        """
        Actions to be performed when running and receives an order pending update event.

        Parameters
        ----------
        event : OrderPendingUpdate
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_pending_cancel(self, OrderPendingCancel event):
        """
        Actions to be performed when running and receives an order pending cancel event.

        Parameters
        ----------
        event : OrderPendingCancel
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_modify_rejected(self, OrderModifyRejected event):
        """
        Actions to be performed when running and receives an order modify rejected event.

        Parameters
        ----------
        event : OrderModifyRejected
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_cancel_rejected(self, OrderCancelRejected event):
        """
        Actions to be performed when running and receives an order cancel rejected event.

        Parameters
        ----------
        event : OrderCancelRejected
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_updated(self, OrderUpdated event):
        """
        Actions to be performed when running and receives an order updated event.

        Parameters
        ----------
        event : OrderUpdated
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_order_filled(self, OrderFilled event):
        """
        Actions to be performed when running and receives an order filled event.

        Parameters
        ----------
        event : OrderFilled
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_position_event(self, PositionEvent event):
        """
        Actions to be performed when running and receives a position event.

        Parameters
        ----------
        event : PositionEvent
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_position_opened(self, PositionOpened event):
        """
        Actions to be performed when running and receives a position opened event.

        Parameters
        ----------
        event : PositionOpened
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_position_changed(self, PositionChanged event):
        """
        Actions to be performed when running and receives a position changed event.

        Parameters
        ----------
        event : PositionChanged
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass

    cpdef void on_position_closed(self, PositionClosed event):
        """
        Actions to be performed when running and receives a position closed event.

        Parameters
        ----------
        event : PositionClosed
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        # Optionally override in subclass


# -- TRADING COMMANDS -----------------------------------------------------------------------------

    cpdef MarketOrder spawn_market(
        self,
        Order primary,
        Quantity quantity,
        TimeInForce time_in_force = TimeInForce.GTC,
        bint reduce_only = False,
        list[str] tags = None,
        bint reduce_primary = True,
    ):
        """
        Spawn a new ``MARKET`` order from the given primary order.

        Parameters
        ----------
        primary : Order
            The primary order from which this order will spawn.
        quantity : Quantity
            The spawned orders quantity (> 0).
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
            The spawned orders time in force. Often not applicable for market orders.
        reduce_only : bool, default False
            If the spawned order carries the 'reduce-only' execution instruction.
        tags : list[str], optional
            The custom user tags for the order.
        reduce_primary : bool, default True
            If the primary order quantity should be reduced by the given `quantity`.

        Returns
        -------
        MarketOrder

        Raises
        ------
        ValueError
            If `primary.exec_algorithm_id` is not equal to `self.id`.
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD``.

        """
        Condition.not_none(primary, "primary")
        Condition.not_none(quantity, "quantity")
        Condition.equal(primary.exec_algorithm_id, self.id, "primary.exec_algorithm_id", "id")

        if reduce_primary:
            self._reduce_primary_order(primary, spawn_qty=quantity)

        return MarketOrder(
            trader_id=primary.trader_id,
            strategy_id=primary.strategy_id,
            instrument_id=primary.instrument_id,
            client_order_id=self._spawn_client_order_id(primary),
            order_side=primary.side,
            quantity=quantity,
            time_in_force=time_in_force,
            reduce_only=reduce_only,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            contingency_type=primary.contingency_type,
            order_list_id=primary.order_list_id,
            linked_order_ids=primary.linked_order_ids,
            parent_order_id=primary.parent_order_id,
            exec_algorithm_id=self.id,
            exec_spawn_id=primary.client_order_id,
            tags=tags,
        )

    cpdef LimitOrder spawn_limit(
        self,
        Order primary,
        Quantity quantity,
        Price price,
        TimeInForce time_in_force = TimeInForce.GTC,
        datetime expire_time = None,
        bint post_only = False,
        bint reduce_only = False,
        Quantity display_qty = None,
        TriggerType emulation_trigger = TriggerType.NO_TRIGGER,
        list[str] tags = None,
        bint reduce_primary = True,
    ):
        """
        Spawn a new ``LIMIT`` order from the given primary order.

        Parameters
        ----------
        primary : Order
            The primary order from which this order will spawn.
        quantity : Quantity
            The spawned orders quantity (> 0). Must be less than `primary.quantity`.
        price : Price
            The spawned orders price.
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
            The spawned orders time in force.
        expire_time : datetime, optional
            The spawned order expiration (for ``GTD`` orders).
        post_only : bool, default False
            If the spawned order will only provide liquidity (make a market).
        reduce_only : bool, default False
            If the spawned order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the spawned order to display on the public book (iceberg).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The type of market price trigger to use for local order emulation.
            - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
            - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
            Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
        tags : list[str], optional
            The custom user tags for the order.
        reduce_primary : bool, default True
            If the primary order quantity should be reduced by the given `quantity`.

        Returns
        -------
        LimitOrder

        Raises
        ------
        ValueError
            If `primary.exec_algorithm_id` is not equal to `self.id`.
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        Condition.not_none(primary, "primary")
        Condition.not_none(quantity, "quantity")
        Condition.equal(primary.exec_algorithm_id, self.id, "primary.exec_algorithm_id", "id")

        if reduce_primary:
            self._reduce_primary_order(primary, spawn_qty=quantity)

        return LimitOrder(
            trader_id=primary.trader_id,
            strategy_id=primary.strategy_id,
            instrument_id=primary.instrument_id,
            client_order_id=self._spawn_client_order_id(primary),
            order_side=primary.side,
            quantity=quantity,
            price=price,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            time_in_force=time_in_force,
            expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
            post_only=post_only,
            reduce_only=reduce_only,
            display_qty=display_qty,
            emulation_trigger=emulation_trigger,
            contingency_type=primary.contingency_type,
            order_list_id=primary.order_list_id,
            linked_order_ids=primary.linked_order_ids,
            parent_order_id=primary.parent_order_id,
            exec_algorithm_id=self.id,
            exec_spawn_id=primary.client_order_id,
            tags=tags,
        )

    cpdef MarketToLimitOrder spawn_market_to_limit(
        self,
        Order primary,
        Quantity quantity,
        TimeInForce time_in_force = TimeInForce.GTC,
        datetime expire_time = None,
        bint reduce_only = False,
        Quantity display_qty = None,
        TriggerType emulation_trigger = TriggerType.NO_TRIGGER,
        list[str] tags = None,
        bint reduce_primary = True,
    ):
        """
        Spawn a new ``MARKET_TO_LIMIT`` order from the given primary order.

        Parameters
        ----------
        primary : Order
            The primary order from which this order will spawn.
        quantity : Quantity
            The spawned orders quantity (> 0). Must be less than `primary.quantity`.
        time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
            The spawned orders time in force.
        expire_time : datetime, optional
            The spawned order expiration (for ``GTD`` orders).
        reduce_only : bool, default False
            If the spawned order carries the 'reduce-only' execution instruction.
        display_qty : Quantity, optional
            The quantity of the spawned order to display on the public book (iceberg).
        emulation_trigger : TriggerType, default ``NO_TRIGGER``
            The type of market price trigger to use for local order emulation.
            - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
            - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
            Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
        tags : list[str], optional
            The custom user tags for the order.
        reduce_primary : bool, default True
            If the primary order quantity should be reduced by the given `quantity`.

        Returns
        -------
        MarketToLimitOrder

        Raises
        ------
        ValueError
            If `primary.exec_algorithm_id` is not equal to `self.id`.
        ValueError
            If `quantity` is not positive (> 0).
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        Condition.not_none(primary, "primary")
        Condition.not_none(quantity, "quantity")
        Condition.equal(primary.exec_algorithm_id, self.id, "primary.exec_algorithm_id", "id")

        if reduce_primary:
            self._reduce_primary_order(primary, spawn_qty=quantity)

        return MarketToLimitOrder(
            trader_id=primary.trader_id,
            strategy_id=primary.strategy_id,
            instrument_id=primary.instrument_id,
            client_order_id=self._spawn_client_order_id(primary),
            order_side=primary.side,
            quantity=quantity,
            reduce_only=reduce_only,
            display_qty=display_qty,
            init_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            time_in_force=time_in_force,
            expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
            contingency_type=primary.contingency_type,
            order_list_id=primary.order_list_id,
            linked_order_ids=primary.linked_order_ids,
            parent_order_id=primary.parent_order_id,
            exec_algorithm_id=self.id,
            exec_spawn_id=primary.client_order_id,
            tags=tags,
        )

    cpdef void submit_order(self, Order order):
        """
        Submit the given order (may be the primary or spawned order).

        A `SubmitOrder` command will be created and sent to the `RiskEngine`.

        If the client order ID is duplicate, then the order will be denied.

        Parameters
        ----------
        order : Order
            The order to submit.
        parent_order_id : ClientOrderId, optional
            The parent client order identifier. If provided then will be considered a child order
            of the parent.

        Raises
        ------
        ValueError
            If `order.status` is not ``INITIALIZED`` or ``RELEASED``.
        ValueError
            If `order.emulation_trigger` is not ``NO_TRIGGER``.

        Warning
        -------
        If a `position_id` is passed and a position does not yet exist, then any
        position opened by the order will have this position ID assigned. This may
        not be what you intended.

        Emulated orders cannot be sent from execution algorithms (intentionally constraining complexity).

        """
        Condition.is_true(self.trader_id is not None, "The execution algorithm has not been registered")
        Condition.not_none(order, "order")
        Condition.equal(order.emulation_trigger, TriggerType.NO_TRIGGER, "order.emulation_trigger", "NO_TRIGGER")
        Condition.is_true(
            order.status_c() in (OrderStatus.INITIALIZED, OrderStatus.RELEASED),
            "order",
            "order status was not either ``INITIALIZED`` or ``RELEASED``",
        )

        cdef Order primary = None
        cdef PositionId position_id = None
        cdef ClientId client_id = None
        cdef SubmitOrder command = None

        if order.is_spawned_c():
            # Handle new spawned order
            primary = self.cache.order(order.exec_spawn_id)
            Condition.equal(order.strategy_id, primary.strategy_id, "order.strategy_id", "primary.strategy_id")
            if primary is None:
                self._log.error(
                    f"Cannot submit order: cannot find primary order for {order.exec_spawn_id!r}"
                )
                return

            position_id = self.cache.position_id(primary.client_order_id)
            client_id = self.cache.client_id(primary.client_order_id)

            if self.cache.order_exists(order.client_order_id):
                self._log.error(
                    f"Cannot submit order: order already exists for {order.client_order_id!r}",
                )
                return

            # Publish initialized event
            self._msgbus.publish_c(
                topic=f"events.order.{order.strategy_id.to_str()}",
                msg=order.init_event_c(),
            )

            self.cache.add_order(order, position_id)

            command = SubmitOrder(
                trader_id=self.trader_id,
                strategy_id=primary.strategy_id,
                order=order,
                command_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
                position_id=primary.position_id,
                client_id=client_id,
            )

            self._send_risk_command(command)
            return

        # Handle primary (original) order
        position_id = self.cache.position_id(order.client_order_id)
        client_id = self.cache.client_id(order.client_order_id)
        cdef Order cached_order = self.cache.order(order.client_order_id)
        if cached_order.order_type != order.order_type:
            self.cache.add_order(order, position_id, client_id, overwrite=True)

        command = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=order.strategy_id,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            position_id=position_id,
            client_id=client_id,
        )

        self._send_risk_command(command)

    cpdef void modify_order(
        self,
        Order order,
        Quantity quantity = None,
        Price price = None,
        Price trigger_price = None,
        ClientId client_id = None,
    ):
        """
        Modify the given order with optional parameters and routing instructions.

        An `ModifyOrder` command will be created and then sent to the `RiskEngine`.

        At least one value must differ from the original order for the command to be valid.

        Will use an Order Cancel/Replace Request (a.k.a Order Modification)
        for FIX protocols, otherwise if order update is not available for
        the API, then will cancel and replace with a new order using the
        original `ClientOrderId`.

        Parameters
        ----------
        order : Order
            The order to update.
        quantity : Quantity, optional
            The updated quantity for the given order.
        price : Price, optional
            The updated price for the given order (if applicable).
        trigger_price : Price, optional
            The updated trigger price for the given order (if applicable).
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        Raises
        ------
        ValueError
            If `price` is not ``None`` and order does not have a `price`.
        ValueError
            If `trigger` is not ``None`` and order does not have a `trigger_price`.

        Warnings
        --------
        If the order is already closed or at `PENDING_CANCEL` status
        then the command will not be generated, and a warning will be logged.

        References
        ----------
        https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_G_71.html

        """
        Condition.is_true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(order, "order")

        cdef bint updating = False  # Set validation flag (must become true)

        if quantity is not None and quantity != order.quantity:
            updating = True

        if price is not None:
            Condition.is_true(
                order.order_type in LIMIT_ORDER_TYPES,
                fail_msg=f"{order.type_string_c()} orders do not have a LIMIT price",
            )
            if price != order.price:
                updating = True

        if trigger_price is not None:
            Condition.is_true(
                order.order_type in STOP_ORDER_TYPES,
                fail_msg=f"{order.type_string_c()} orders do not have a STOP trigger price",
            )
            if trigger_price != order.trigger_price:
                updating = True

        if not updating:
            self.log.error(
                "Cannot create command ModifyOrder: "
                "quantity, price and trigger were either None "
                "or the same as existing values.",
            )
            return

        if order.is_closed_c() or order.is_pending_cancel_c():
            self.log.warning(
                f"Cannot create command ModifyOrder: "
                f"state is {order.status_string_c()}, {order}.",
            )
            return  # Cannot send command

        cdef OrderPendingUpdate event
        if not order.is_active_local_c():
            # Generate and apply event
            event = self._generate_order_pending_update(order)
            try:
                order.apply(event)
            except InvalidStateTrigger as e:  # pragma: no cover
                self._log.warning(f"InvalidStateTrigger: {e}, did not apply {event}")
                return

            self.cache.update_order(order)

            # Publish event
            self._msgbus.publish_c(
                topic=f"events.order.{order.strategy_id.to_str()}",
                msg=event,
            )

        cdef ModifyOrder command = ModifyOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
        )

        self._send_risk_command(command)

    cpdef void modify_order_in_place(
        self,
        Order order,
        Quantity quantity = None,
        Price price = None,
        Price trigger_price = None,
    ):
        """
        Modify the given ``INITIALIZED`` order in place (immediately) with optional parameters.

        At least one value must differ from the original order for the command to be valid.

        Parameters
        ----------
        order : Order
            The order to update.
        quantity : Quantity, optional
            The updated quantity for the given order.
        price : Price, optional
            The updated price for the given order (if applicable).
        trigger_price : Price, optional
            The updated trigger price for the given order (if applicable).

        Raises
        ------
        ValueError
            If `order.status` is not ``INITIALIZED`` or ``RELEASED``.
        ValueError
            If `price` is not ``None`` and order does not have a `price`.
        ValueError
            If `trigger` is not ``None`` and order does not have a `trigger_price`.

        Warnings
        --------
        If the order is already closed or at `PENDING_CANCEL` status
        then the command will not be generated, and a warning will be logged.

        References
        ----------
        https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_G_71.html

        """
        Condition.is_true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(order, "order")
        Condition.is_true(
            order.status_c() in (OrderStatus.INITIALIZED, OrderStatus.RELEASED),
            "order",
            "order status was not either ``INITIALIZED`` or ``RELEASED``",
        )

        cdef bint updating = False  # Set validation flag (must become true)

        if quantity is not None and quantity != order.quantity:
            updating = True

        if price is not None:
            Condition.is_true(
                order.order_type in LIMIT_ORDER_TYPES,
                fail_msg=f"{order.type_string_c()} orders do not have a LIMIT price",
            )
            if price != order.price:
                updating = True

        if trigger_price is not None:
            Condition.is_true(
                order.order_type in STOP_ORDER_TYPES,
                fail_msg=f"{order.type_string_c()} orders do not have a STOP trigger price",
            )
            if trigger_price != order.trigger_price:
                updating = True

        if not updating:
            self.log.error(
                "Cannot create command ModifyOrder: "
                "quantity, price and trigger were either None "
                "or the same as existing values.",
            )
            return

        if order.is_closed_c() or order.is_pending_cancel_c():
            self.log.warning(
                f"Cannot create command ModifyOrder: "
                f"state is {order.status_string_c()}, {order}.",
            )
            return  # Cannot send command

        # Generate event
        cdef uint64_t ts_now = self._clock.timestamp_ns()

        cdef OrderUpdated updated = OrderUpdated(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id,
            quantity=quantity or order.quantity,
            price=price,
            trigger_price=trigger_price,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )

        order.apply(updated)
        self.cache.update_order(order)

    cpdef void cancel_order(self, Order order, ClientId client_id = None):
        """
        Cancel the given order with optional routing instructions.

        A `CancelOrder` command will be created and then sent to **either** the
        `OrderEmulator` or the `ExecutionEngine` (depending on whether the order is emulated).

        Logs an error if no `VenueOrderId` has been assigned to the order.

        Parameters
        ----------
        order : Order
            The order to cancel.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.

        """
        Condition.is_true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(order, "order")

        if order.is_closed_c() or order.is_pending_cancel_c():
            self.log.warning(
                f"Cannot cancel order: state is {order.status_string_c()}, {order}.",
            )
            return  # Cannot send command

        cdef OrderPendingCancel event
        if not order.is_active_local_c():
            # Generate and apply event
            event = self._generate_order_pending_cancel(order)
            try:
                order.apply(event)
            except InvalidStateTrigger as e:  # pragma: no cover
                self._log.warning(f"InvalidStateTrigger: {e}, did not apply {event}")
                return

            self.cache.update_order(order)

            # Publish event
            self._msgbus.publish_c(
                topic=f"events.order.{order.strategy_id.to_str()}",
                msg=event,
            )

        cdef CancelOrder command = CancelOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
        )

        if order.is_emulated_c() or order.status_c() == OrderStatus.RELEASED:
            self._send_emulator_command(command)
        else:
            self._send_exec_command(command)

# -- EVENTS ---------------------------------------------------------------------------------------

    cdef OrderPendingUpdate _generate_order_pending_update(self, Order order):
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        return OrderPendingUpdate(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )

    cdef OrderPendingCancel _generate_order_pending_cancel(self, Order order):
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        return OrderPendingCancel(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )

    cdef OrderCanceled _generate_order_canceled(self, Order order):
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        return OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )

# -- EGRESS ---------------------------------------------------------------------------------------

    cdef void _send_emulator_command(self, TradingCommand command):
        if self._log_commands and is_logging_initialized():
            self.log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="OrderEmulator.execute", msg=command)

    cdef void _send_risk_command(self, TradingCommand command):
        if self._log_commands and is_logging_initialized():
            self.log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="RiskEngine.execute", msg=command)

    cdef void _send_exec_command(self, TradingCommand command):
        if self._log_commands and is_logging_initialized():
            self.log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="ExecEngine.execute", msg=command)
