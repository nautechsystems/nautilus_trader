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

"""
This module defines a trading strategy class which allows users to implement
their own customized trading strategies

A user can inherit from `Strategy` and optionally override any of the
"on" named event methods. The class is not entirely initialized in a stand-alone
way, the intended usage is to pass strategies to a `Trader` so that they can be
fully "wired" into the platform. Exceptions will be raised if a `Strategy`
attempts to operate without a managing `Trader` instance.

"""

from nautilus_trader.trading.config import ImportableStrategyConfig
from nautilus_trader.trading.config import StrategyConfig

from libc.stdint cimport uint64_t

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.component cimport EVT
from nautilus_trader.common.component cimport RECV
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport LogColor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.common cimport ComponentState
from nautilus_trader.core.rust.model cimport OrderStatus
from nautilus_trader.core.rust.model cimport TimeInForce
from nautilus_trader.core.rust.model cimport TriggerType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.messages cimport BatchCancelOrders
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport QueryOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
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
from nautilus_trader.model.functions cimport oms_type_from_str
from nautilus_trader.model.functions cimport order_side_to_str
from nautilus_trader.model.functions cimport order_status_to_str
from nautilus_trader.model.functions cimport position_side_to_str
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport LIMIT_ORDER_TYPES
from nautilus_trader.model.orders.base cimport STOP_ORDER_TYPES
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class Strategy(Actor):
    """
    The base class for all trading strategies.

    This class allows traders to implement their own customized trading strategies.
    A trading strategy can configure its own order management system type, which
    determines how positions are handled by the `ExecutionEngine`.

    Strategy OMS (Order Management System) types:
     - ``UNSPECIFIED``: No specific type has been configured, will therefore
       default to the native OMS type for each venue.
     - ``HEDGING``: A position ID will be assigned for each new position which
       is opened per instrument.
     - ``NETTING``: There will only be a single position for the strategy per
       instrument. The position ID naming convention is `{instrument_id}-{strategy_id}`.

    Parameters
    ----------
    config : StrategyConfig, optional
        The trading strategy configuration.

    Raises
    ------
    TypeError
        If `config` is not of type `StrategyConfig`.

    Warnings
    --------
    - This class should not be used directly, but through a concrete subclass.
    - Do not call components such as `clock` and `logger` in the `__init__` prior to registration.
    """

    def __init__(self, config: StrategyConfig | None = None):
        if config is None:
            config = StrategyConfig()
        Condition.type(config, StrategyConfig, "config")

        super().__init__()
        # Assign strategy ID after base class initialized
        component_id = type(self).__name__ if config.strategy_id is None else config.strategy_id
        self.id = StrategyId(f"{component_id}-{config.order_id_tag}")
        self.order_id_tag = str(config.order_id_tag)
        self.use_uuid_client_order_ids = config.use_uuid_client_order_ids
        self._log = Logger(name=component_id)

        oms_type = config.oms_type or OmsType.UNSPECIFIED
        if isinstance(oms_type, str):
            oms_type = oms_type_from_str(config.oms_type.upper())

        # Configuration
        self._log_events = config.log_events
        self._log_commands = config.log_commands
        self.config = config
        self.oms_type = <OmsType>oms_type
        self.external_order_claims = self._parse_external_order_claims(config.external_order_claims)
        self.manage_contingent_orders = config.manage_contingent_orders
        self.manage_gtd_expiry = config.manage_gtd_expiry

        # Public components
        self.clock = self._clock
        self.cache: Cache = None   # Initialized when registered
        self.portfolio = None      # Initialized when registered
        self.order_factory = None  # Initialized when registered

        # Order management
        self._manager = None       # Initialized when registered

        # Register warning events
        self.register_warning_event(OrderDenied)
        self.register_warning_event(OrderRejected)
        self.register_warning_event(OrderCancelRejected)
        self.register_warning_event(OrderModifyRejected)

    def _parse_external_order_claims(
        self,
        config_claims: list[str] | None,
    ) -> list[InstrumentId]:
        if config_claims is None:
            return []

        order_claims: list[InstrumentId] = []
        for instrument_id in config_claims:
            if isinstance(instrument_id, str):
                instrument_id = InstrumentId.from_str(instrument_id)
            order_claims.append(instrument_id)

        return order_claims

    def to_importable_config(self) -> ImportableStrategyConfig:
        """
        Returns an importable configuration for this strategy.

        Returns
        -------
        ImportableStrategyConfig

        """
        return ImportableStrategyConfig(
            strategy_path=self.fully_qualified_name(),
            config_path=self.config.fully_qualified_name(),
            config=self.config.dict(),
        )

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void on_start(self):
        # Should override in subclass
        self.log.warning(
            "The `Strategy.on_start` handler was called when not overridden. "
            "It's expected that any actions required when starting the strategy "
            "occur here, such as subscribing/requesting data",
        )

    cpdef void on_stop(self):
        # Should override in subclass
        self.log.warning(
            "The `Strategy.on_stop` handler was called when not overridden. "
            "It's expected that any actions required when stopping the strategy "
            "occur here, such as unsubscribing from data",
        )

    cpdef void on_resume(self):
        # Should override in subclass
        self.log.warning(
            "The `Strategy.on_resume` handler was called when not overridden. "
            "It's expected that any actions required when resuming the strategy "
            "following a stop occur here"
        )

    cpdef void on_reset(self):
        # Should override in subclass
        self.log.warning(
            "The `Strategy.on_reset` handler was called when not overridden. "
            "It's expected that any actions required when resetting the strategy "
            "occur here, such as resetting indicators and other state"
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
        Register the strategy with a trader.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID for the strategy.
        portfolio : PortfolioFacade
            The read-only portfolio for the strategy.
        msgbus : MessageBus
            The message bus for the strategy.
        cache : CacheFacade
            The read-only cache for the strategy.
        clock : Clock
            The clock for the strategy.

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

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.id,
            clock=clock,
            cache=cache,
            use_uuid_client_order_ids=self.use_uuid_client_order_ids
        )

        self._manager = OrderManager(
            clock=clock,
            msgbus=msgbus,
            cache=cache,
            component_name=type(self).__name__,
            active_local=False,
            submit_order_handler=None,
            cancel_order_handler=self.cancel_order,
            modify_order_handler=self.modify_order,
            debug=False,  # Set True for debugging
            log_events=self._log_events,
            log_commands=self._log_commands,
        )

        # Required subscriptions
        self._msgbus.subscribe(topic=f"events.order.{self.id}", handler=self.handle_event)
        self._msgbus.subscribe(topic=f"events.position.{self.id}", handler=self.handle_event)

    cpdef void change_id(self, StrategyId strategy_id):
        """
        Change the strategies identifier to the given `strategy_id`.

        Parameters
        ----------
        strategy_id : StrategyId
            The new strategy ID to change to.

        """
        Condition.not_none(strategy_id, "strategy_id")

        self.id = strategy_id

    cpdef void change_order_id_tag(self, str order_id_tag):
        """
        Change the order identifier tag to the given `order_id_tag`.

        Parameters
        ----------
        order_id_tag : str
            The new order ID tag to change to.

        """
        Condition.valid_string(order_id_tag, "order_id_tag")

        self.order_id_tag = order_id_tag

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _start(self):
        # Log configuration
        self._log.info(f"{self.config.oms_type=}", LogColor.BLUE)
        self._log.info(f"{self.config.external_order_claims=}", LogColor.BLUE)
        self._log.info(f"{self.config.manage_gtd_expiry=}", LogColor.BLUE)

        cdef set client_order_ids = self.cache.client_order_ids(
            venue=None,
            instrument_id=None,
            strategy_id=self.id,
        )

        cdef set order_list_ids = self.cache.order_list_ids(
            venue=None,
            instrument_id=None,
            strategy_id=self.id,
        )

        cdef int order_id_count = len(client_order_ids)
        cdef int order_list_id_count = len(order_list_ids)
        self.order_factory.set_client_order_id_count(order_id_count)
        self.log.info(
            f"Set ClientOrderIdGenerator client_order_id count to {order_id_count}",
            LogColor.BLUE,
        )
        self.order_factory.set_order_list_id_count(order_list_id_count)
        self.log.info(
            f"Set ClientOrderIdGenerator order_list_id count to {order_list_id_count}",
            LogColor.BLUE,
        )

        cdef list open_orders = self.cache.orders_open(
            venue=None,
            instrument_id=None,
            strategy_id=self.id,
        )

        if self.manage_gtd_expiry:
            for order in open_orders:
                if order.time_in_force == TimeInForce.GTD:
                    if self._clock.timestamp_ns() >= order.expire_time_ns:
                        self.cancel_order(order)
                        continue
                    if not self._has_gtd_expiry_timer(order.client_order_id):
                        self._set_gtd_expiry(order)

        self.on_start()

    cpdef void _reset(self):
        if self.order_factory:
            self.order_factory.reset()

        self._indicators.clear()
        self._indicators_for_quotes.clear()
        self._indicators_for_trades.clear()
        self._indicators_for_bars.clear()

        if self._manager:
            self._manager.reset()

        self.on_reset()

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

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
        Actions to be performed when running and receives an order emulated event.

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

    cpdef void submit_order(
        self,
        Order order,
        PositionId position_id = None,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Submit the given order with optional position ID, execution algorithm
        and routing instructions.

        A `SubmitOrder` command will be created and sent to **either** an
        `ExecAlgorithm`, the `OrderEmulator` or the `RiskEngine` (depending whether
        the order is emulated and/or has an `exec_algorithm_id` specified).

        If the client order ID is duplicate, then the order will be denied.

        Parameters
        ----------
        order : Order
            The order to submit.
        position_id : PositionId, optional
            The position ID to submit the order against. If a position does not
            yet exist, then any position opened will have this identifier assigned.
        client_id : ClientId, optional
            The specific execution client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Raises
        ------
        ValueError
            If `order.status` is not ``INITIALIZED``.

        Warning
        -------
        If a `position_id` is passed and a position does not yet exist, then any
        position opened by the order will have this position ID assigned. This may
        not be what you intended.

        """
        Condition.is_true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(order, "order")
        if order._fsm.state != OrderStatus.INITIALIZED:  # Check predicate first for efficiency
            Condition.is_true(
                order.status_c() == OrderStatus.INITIALIZED,
                f"Invalid order status on submit: expected 'INITIALIZED', was '{order.status_string_c()}'",
            )

        # Publish initialized event
        self._msgbus.publish_c(
            topic=f"events.order.{order.strategy_id.to_str()}",
            msg=order.init_event_c(),
        )

        # Check for duplicate client order ID
        if self.cache.order_exists(order.client_order_id):
            self._deny_order(order, f"duplicate {repr(order.client_order_id)}")
            return

        self.cache.add_order(order, position_id, client_id)

        cdef SubmitOrder command = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            order=order,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            position_id=position_id,
            client_id=client_id,
            params=params,
        )

        if self.manage_gtd_expiry and order.time_in_force == TimeInForce.GTD:
            self._set_gtd_expiry(order)

        # Route order
        if order.emulation_trigger != TriggerType.NO_TRIGGER:
            self._manager.send_emulator_command(command)
        elif order.exec_algorithm_id is not None:
            self._manager.send_algo_command(command, order.exec_algorithm_id)
        else:
            self._manager.send_risk_command(command)

    cpdef void submit_order_list(
        self,
        OrderList order_list,
        PositionId position_id = None,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Submit the given order list with optional position ID, execution algorithm
        and routing instructions.

        A `SubmitOrderList` command will be created and sent to **either** the
        `OrderEmulator`, or the `RiskEngine` (depending whether an order is emulated).

        If the order list ID is duplicate, or any client order ID is duplicate,
        then all orders will be denied.

        Parameters
        ----------
        order_list : OrderList
            The order list to submit.
        position_id : PositionId, optional
            The position ID to submit the order against. If a position does not
            yet exist, then any position opened will have this identifier assigned.
        client_id : ClientId, optional
            The specific execution client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Raises
        ------
        ValueError
            If any `order.status` is not ``INITIALIZED``.

        Warning
        -------
        If a `position_id` is passed and a position does not yet exist, then any
        position opened by an order will have this position ID assigned. This may
        not be what you intended.

        """
        Condition.is_true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(order_list, "order_list")

        cdef Order order
        for order in order_list.orders:
            Condition.equal(order.status_c(), OrderStatus.INITIALIZED, "order", "order_status")
            # Publish initialized event
            self._msgbus.publish_c(
                topic=f"events.order.{order.strategy_id.to_str()}",
                msg=order.init_event_c(),
            )

        # Check for duplicate order list ID
        if self.cache.order_list_exists(order_list.id):
            self._deny_order_list(
                order_list,
                reason=f"duplicate {repr(order_list.id)}",
            )
            return

        self.cache.add_order_list(order_list)

        # Check for duplicate client order IDs
        for order in order_list.orders:
            if self.cache.order_exists(order.client_order_id):
                for order in order_list.orders:
                    self._deny_order(
                        order,
                        reason=f"duplicate {repr(order.client_order_id)}",
                    )
                return

        for order in order_list.orders:
            self.cache.add_order(order, position_id, client_id)

        cdef SubmitOrderList command = SubmitOrderList(
            trader_id=self.trader_id,
            strategy_id=self.id,
            order_list=order_list,
            position_id=position_id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
            params=params,
        )

        if self.manage_gtd_expiry:
            for order in command.order_list.orders:
                if order.time_in_force == TimeInForce.GTD:
                    self._set_gtd_expiry(order)

        # Route order
        if command.has_emulated_order:
            self._manager.send_emulator_command(command)
        elif order_list.first.exec_algorithm_id is not None:
            self._manager.send_algo_command(command, order_list.first.exec_algorithm_id)
        else:
            self._manager.send_risk_command(command)

    cpdef void modify_order(
        self,
        Order order,
        Quantity quantity = None,
        Price price = None,
        Price trigger_price = None,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Modify the given order with optional parameters and routing instructions.

        An `ModifyOrder` command will be created and then sent to **either** the
        `OrderEmulator` or the `RiskEngine` (depending on whether the order is emulated).

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
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

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

        cdef ModifyOrder command = self._create_modify_order(
            order=order,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            client_id=client_id,
            params=params,
        )
        if command is None:
            return

        if order.is_emulated_c():
            self._manager.send_emulator_command(command)
        else:
            self._manager.send_risk_command(command)

    cpdef void cancel_order(self, Order order, ClientId client_id = None, dict[str, object] params = None):
        """
        Cancel the given order with optional routing instructions.

        A `CancelOrder` command will be created and then sent to **either** the
        `OrderEmulator` or the `ExecutionEngine` (depending on whether the order is emulated).

        Parameters
        ----------
        order : Order
            The order to cancel.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.is_true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(order, "order")

        cdef CancelOrder command = self._create_cancel_order(
            order=order,
            client_id=client_id,
            params=params,
        )
        if command is None:
            return

        if order.is_emulated_c() or order.emulation_trigger != TriggerType.NO_TRIGGER:
            self._manager.send_emulator_command(command)
        elif order.exec_algorithm_id is not None and order.is_active_local_c():
            self._manager.send_algo_command(command, order.exec_algorithm_id)
        else:
            self._manager.send_exec_command(command)

        # Cancel any GTD expiry timer
        if self.manage_gtd_expiry:
            if order.time_in_force == TimeInForce.GTD and self._has_gtd_expiry_timer(order.client_order_id):
                self.cancel_gtd_expiry(order)

    cpdef void cancel_orders(self, list orders, ClientId client_id = None, dict[str, object] params = None):
        """
        Batch cancel the given list of orders with optional routing instructions.

        For each order in the list, a `CancelOrder` command will be created and added to a
        `BatchCancelOrders` command. This command is then sent to the `ExecutionEngine`.

        Logs an error if the `orders` list contains local/emulated orders.

        Parameters
        ----------
        orders : list[Order]
            The orders to cancel.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        Raises
        ------
        ValueError
            If `orders` is empty.
        TypeError
            If `orders` contains a type other than `Order`.

        """
        Condition.not_empty(orders, "orders")
        Condition.list_type(orders, Order, "orders")

        cdef list cancels = []

        cdef:
            Order order
            Order first = None
            CancelOrder cancel
        for order in orders:
            if first is None:
                first = order
            else:
                if first.instrument_id != order.instrument_id:
                    self._log.error(
                        "Cannot cancel all orders: instrument_id mismatch "
                        f"{first.instrument_id} vs {order.instrument_id}",
                    )
                    return
                if order.is_emulated_c():
                    self._log.error(
                        "Cannot include emulated orders in a batch cancel"
                    )
                    return

            cancel = self._create_cancel_order(
                order=order,
                client_id=client_id,
            )
            if cancel is None:
                continue
            cancels.append(cancel)

        if not cancels:
            self._log.warning("Cannot send `BatchCancelOrders`, no valid cancel commands")
            return

        cdef command = BatchCancelOrders(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=first.instrument_id,
            cancels=cancels,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
            params=params,
        )

        self._manager.send_exec_command(command)

    cpdef void cancel_all_orders(
        self,
        InstrumentId instrument_id,
        OrderSide order_side = OrderSide.NO_ORDER_SIDE,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
        """
        Cancel all orders for this strategy for the given instrument ID.

        A `CancelAllOrders` command will be created and then sent to **both** the
        `OrderEmulator` and the `ExecutionEngine`.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the orders to cancel.
        order_side : OrderSide, default ``NO_ORDER_SIDE`` (both sides)
            The side of the orders to cancel.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.is_true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(instrument_id, "instrument_id")

        cdef list open_orders = self.cache.orders_open(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=self.id,
            side=order_side,
        )

        cdef list emulated_orders = self.cache.orders_emulated(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=self.id,
            side=order_side,
        )

        cdef str order_side_str = " " + order_side_to_str(order_side) if order_side != OrderSide.NO_ORDER_SIDE else ""
        if not open_orders and not emulated_orders:
            self.log.info(
                f"No {instrument_id.to_str()} open or emulated{order_side_str} "
                f"orders to cancel")
            return

        cdef int open_count = len(open_orders)
        if open_count:
            self.log.info(
                f"Canceling {open_count} open{order_side_str} "
                f"{instrument_id.to_str()} order{'' if open_count == 1 else 's'}",
            )

        cdef int emulated_count = len(emulated_orders)
        if emulated_count:
            self.log.info(
                f"Canceling {emulated_count} emulated{order_side_str} "
                f"{instrument_id.to_str()} order{'' if emulated_count == 1 else 's'}",
            )

        cdef:
            OrderPendingCancel event
            Order order
        for order in open_orders + emulated_orders:
            if order.status_c() == OrderStatus.INITIALIZED or order.is_emulated_c():
                continue
            event = self._generate_order_pending_cancel(order)
            try:
                order.apply(event)
            except InvalidStateTrigger as e:
                self._log.warning(f"InvalidStateTrigger: {e}, did not apply {event}")
                continue

            self.cache.update_order(order)

        cdef CancelAllOrders command = CancelAllOrders(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=instrument_id,
            order_side=order_side,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
            params=params,
        )

        # Cancel all execution algorithm orders
        cdef set exec_algorithm_ids = self.cache.exec_algorithm_ids()

        cdef:
            ExecAlgorithmId exec_algorithm_id
        for exec_algorithm_id in exec_algorithm_ids:
            exec_algorithm_orders = self.cache.orders_for_exec_algorithm(exec_algorithm_id)
            for order in exec_algorithm_orders:
                if order.strategy_id == self.id and not order.is_closed_c():
                    self.cancel_order(order)

        self._manager.send_exec_command(command)
        self._manager.send_emulator_command(command)

    cpdef void close_position(
        self,
        Position position,
        ClientId client_id = None,
        list[str] tags = None,
        TimeInForce time_in_force = TimeInForce.GTC,
        bint reduce_only = True,
        dict[str, object] params = None,
    ):
        """
        Close the given position.

        A closing `MarketOrder` for the position will be created, and then sent
        to the `ExecutionEngine` via a `SubmitOrder` command.

        Parameters
        ----------
        position : Position
            The position to close.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        tags : list[str], optional
            The tags for the market order closing the position.
        time_in_force : TimeInForce, default ``GTC``
            The time in force for the market order closing the position.
        reduce_only : bool, default True
            If the market order to close the position should carry the 'reduce-only' execution instruction.
            Optional, as not all venues support this feature.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.is_true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(position, "position")
        Condition.not_none(self.trader_id, "self.trader_id")
        Condition.not_none(self.order_factory, "self.order_factory")

        if position.is_closed_c():
            self.log.warning(
                f"Cannot close position "
                f"(the position is already closed), {position}"
            )
            return  # Invalid command

        # Create closing order
        cdef MarketOrder order = self.order_factory.market(
            instrument_id=position.instrument_id,
            order_side=Order.closing_side_c(position.side),
            quantity=position.quantity,
            time_in_force=time_in_force,
            reduce_only=reduce_only,
            quote_quantity=False,
            exec_algorithm_id=None,
            exec_algorithm_params=None,
            tags=tags,
        )

        self.submit_order(order, position_id=position.id, client_id=client_id, params=params)

    cpdef void close_all_positions(
        self,
        InstrumentId instrument_id,
        PositionSide position_side = PositionSide.NO_POSITION_SIDE,
        ClientId client_id = None,
        list[str] tags = None,
        TimeInForce time_in_force = TimeInForce.GTC,
        bint reduce_only = True,
        dict[str, object] params = None,
    ):
        """
        Close all positions for the given instrument ID for this strategy.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the positions to close.
        position_side : PositionSide, default ``NO_POSITION_SIDE`` (both sides)
            The side of the positions to close.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        tags : list[str], optional
            The tags for the market orders closing the positions.
        time_in_force : TimeInForce, default ``GTC``
            The time in force for the market orders closing the positions.
        reduce_only : bool, default True
            If the market orders to close positions should carry the 'reduce-only' execution instruction.
            Optional, as not all venues support this feature.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        # instrument_id can be None
        Condition.is_true(self.trader_id is not None, "The strategy has not been registered")

        cdef list positions_open = self.cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=self.id,
            side=position_side,
        )

        cdef str position_side_str = " " + position_side_to_str(position_side) if position_side != PositionSide.NO_POSITION_SIDE else ""
        if not positions_open:
            self.log.info(
                f"No {instrument_id.to_str()} open{position_side_str} positions to close",
            )
            return

        cdef int count = len(positions_open)
        self.log.info(
            f"Closing {count} open{position_side_str} position{'' if count == 1 else 's'}",
        )

        cdef Position position
        for position in positions_open:
            self.close_position(position, client_id, tags, time_in_force, reduce_only, params)

    cpdef void query_order(self, Order order, ClientId client_id = None, dict[str, object] params = None):
        """
        Query the given order with optional routing instructions.

        A `QueryOrder` command will be created and then sent to the
        `ExecutionEngine`.

        Logs an error if no `VenueOrderId` has been assigned to the order.

        Parameters
        ----------
        order : Order
            The order to query.
        client_id : ClientId, optional
            The specific client ID for the command.
            If ``None`` then will be inferred from the venue in the instrument ID.
        params : dict[str, Any], optional
            Additional parameters potentially used by a specific client.

        """
        Condition.is_true(self.trader_id is not None, "The strategy has not been registered")
        Condition.not_none(order, "order")

        cdef QueryOrder command = QueryOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
            params=params,
        )

        self._manager.send_exec_command(command)

    cdef ModifyOrder _create_modify_order(
        self,
        Order order,
        Quantity quantity = None,
        Price price = None,
        Price trigger_price = None,
        ClientId client_id = None,
        dict[str, object] params = None,
    ):
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
            price_str = f", {order.price=}" if order.has_price_c() else ""
            trigger_str = f", {order.trigger_price=}" if order.has_trigger_price_c() else ""
            self.log.error(
                "Cannot create command ModifyOrder: "
                f"{quantity=}, {price=}, {trigger_price=} were either None "
                f"or the same as existing values: {order.quantity=}{price_str}{trigger_str}",
            )
            return None  # Cannot send command

        if order.is_closed_c() or order.is_pending_cancel_c():
            self.log.warning(
                f"Cannot create command ModifyOrder: "
                f"state is {order.status_string_c()}, {order}",
            )
            return None  # Cannot send command

        cdef OrderPendingUpdate event
        if not order.is_active_local_c():
            # Generate and apply event
            event = self._generate_order_pending_update(order)
            try:
                order.apply(event)
            except InvalidStateTrigger as e:
                self._log.warning(f"InvalidStateTrigger: {e}, did not apply {event}")
                return None  # Cannot send command

            self.cache.update_order(order)

            # Publish event
            self._msgbus.publish_c(
                topic=f"events.order.{order.strategy_id.to_str()}",
                msg=event,
            )

        return ModifyOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            quantity=quantity,
            price=price,
            trigger_price=trigger_price,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
            params=params,
        )

    cdef CancelOrder _create_cancel_order(self, Order order, ClientId client_id = None, dict[str, object] params = None):
        if order.is_closed_c() or order.is_pending_cancel_c():
            self.log.warning(
                f"Cannot cancel order: state is {order.status_string_c()}, {order}",
            )
            return None  # Cannot send command

        cdef OrderStatus order_status = order.status_c()

        cdef OrderPendingCancel event
        if not order.is_active_local_c():
            # Generate and apply event
            event = self._generate_order_pending_cancel(order)
            try:
                order.apply(event)
            except InvalidStateTrigger as e:
                self._log.warning(f"InvalidStateTrigger: {e}, did not apply {event}")
                return None  # Cannot send command

            self.cache.update_order(order)

            # Publish event
            self._msgbus.publish_c(
                topic=f"events.order.{order.strategy_id.to_str()}",
                msg=event,
            )

        return CancelOrder(
            trader_id=self.trader_id,
            strategy_id=self.id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            client_id=client_id,
            params=params,
        )

    cpdef void cancel_gtd_expiry(self, Order order):
        """
        Cancel the managed GTD expiry for the given order.

        If there is no current GTD expiry timer, then an error will be logged.

        Parameters
        ----------
        order : Order
            The order to cancel the GTD expiry for.

        """
        Condition.not_none(order, "order")

        cdef str timer_name = self._get_gtd_expiry_timer_name(order.client_order_id)
        cdef str expire_time_str = f" @ {order.expire_time.isoformat()}" if hasattr(order, "expire_time") else ""

        if timer_name not in self._clock.timer_names:
            self._log.error(f"Cannot find managed GTD timer for order {order.client_order_id!r}")
            return

        self._log.info(
            f"Canceling managed GTD expiry timer for {order.client_order_id}{expire_time_str}",
            LogColor.BLUE,
        )
        self._clock.cancel_timer(name=timer_name)

    cdef str _get_gtd_expiry_timer_name(self, ClientOrderId client_order_id):
        return f"GTD-EXPIRY:{client_order_id.to_str()}"

    cdef bint _has_gtd_expiry_timer(self, ClientOrderId client_order_id):
        cdef str timer_name = self._get_gtd_expiry_timer_name(client_order_id)
        return timer_name in self._clock.timer_names

    cdef void _set_gtd_expiry(self, Order order):
        cdef str timer_name = self._get_gtd_expiry_timer_name(order.client_order_id)
        self._clock.set_time_alert_ns(
            name=timer_name,
            alert_time_ns=order.expire_time_ns,
            callback=self._expire_gtd_order,
        )

        self._log.info(
            f"Set managed GTD expiry timer for {order.client_order_id} @ {order.expire_time.isoformat()}",
            LogColor.BLUE,
        )

    cpdef void _expire_gtd_order(self, TimeEvent event):
        cdef ClientOrderId client_order_id = ClientOrderId(event.to_str().partition(":")[2])
        cdef Order order = self.cache.order(client_order_id)
        if order is None:
            self._log.warning(
                f"Order with {repr(client_order_id)} not found in the cache to apply {event}"
            )

        if order.is_closed_c():
            self._log.warning(f"GTD expired order {order.client_order_id} was already closed")
            return  # Already closed

        self._log.info(f"Expiring GTD order {order.client_order_id}", LogColor.BLUE)
        self.cancel_order(order)

    # -- HANDLERS -------------------------------------------------------------------------------------

    cpdef void handle_event(self, Event event):
        """
        Handle the given event.

        If state is ``RUNNING`` then passes to `on_event`.

        Parameters
        ----------
        event : Event
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(event, "event")

        if type(event) in self._warning_events:
            self.log.warning(f"{RECV}{EVT} {event}")
        elif self._log_events:
            self.log.info(f"{RECV}{EVT} {event}")

        cdef Order order
        if self.manage_gtd_expiry and isinstance(event, OrderEvent):
            order = self.cache.order(event.client_order_id)
            if order is not None and order.is_closed_c() and self._has_gtd_expiry_timer(order.client_order_id):
                self.cancel_gtd_expiry(order)

        if self._fsm.state != ComponentState.RUNNING:
            return

        if self.manage_contingent_orders and self._manager is not None:
            self._manager.handle_event(event)

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

# -- EVENTS ---------------------------------------------------------------------------------------

    cdef OrderDenied _generate_order_denied(self, Order order, str reason):
        cdef uint64_t ts_now = self._clock.timestamp_ns()
        return OrderDenied(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            reason=reason,
            event_id=UUID4(),
            ts_init=ts_now,
        )

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

    cdef void _deny_order(self, Order order, str reason):
        self._log.error(f"Order denied: {reason}")

        if not self.cache.order_exists(order.client_order_id):
            self.cache.add_order(order)

        # Generate event
        cdef OrderDenied event = self._generate_order_denied(order, reason)

        try:
            order.apply(event)
        except InvalidStateTrigger as e:
            self._log.warning(f"InvalidStateTrigger: {e}, did not apply {event}")
            return

        self.cache.update_order(order)

        # Publish denied event
        self._msgbus.publish_c(
            topic=f"events.order.{order.strategy_id.to_str()}",
            msg=event,
        )

    cdef void _deny_order_list(self, OrderList order_list, str reason):
        cdef Order order
        for order in order_list.orders:
            if not order.is_closed_c():
                self._deny_order(order=order, reason=reason)
