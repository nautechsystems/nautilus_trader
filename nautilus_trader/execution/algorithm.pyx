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

from typing import Any, Optional

from nautilus_trader.config import ExecAlgorithmConfig
from nautilus_trader.config import ImportableExecAlgorithmConfig

from cpython.datetime cimport datetime
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport SENT
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.enums_c cimport ContingencyType
from nautilus_trader.model.enums_c cimport OrderStatus
from nautilus_trader.model.enums_c cimport TimeInForce
from nautilus_trader.model.enums_c cimport TriggerType
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.limit cimport LimitOrder
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.orders.market_to_limit cimport MarketToLimitOrder
from nautilus_trader.msgbus.bus cimport MessageBus
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

    def __init__(self, config: Optional[ExecAlgorithmConfig] = None):
        if config is None:
            config = ExecAlgorithmConfig()
        Condition.type(config, ExecAlgorithmConfig, "config")

        super().__init__()
        # Assign Execution Algorithm ID after base class initialized
        component_id = type(self).__name__ if config.exec_algorithm_id is None else config.exec_algorithm_id
        self.id = ExecAlgorithmId(component_id)

        # Configuration
        self.config = config

        self._exec_spawn_ids: dict[ClientOrderId, int] = {}

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
        Logger logger,
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
        logger : Logger
            The logger for the execution algorithm.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(trader_id, "trader_id")
        Condition.not_none(portfolio, "portfolio")
        Condition.not_none(msgbus, "msgbus")
        Condition.not_none(cache, "cache")
        Condition.not_none(clock, "clock")
        Condition.not_none(logger, "logger")

        self.register_base(
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self.portfolio = portfolio

        # Register endpoints
        self._msgbus.register(endpoint=f"{self.id}.execute", handler=self.execute)

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _reset(self):
        self._exec_spawn_ids.clear()

        self.on_reset()

# -- INTERNAL -------------------------------------------------------------------------------------

    cdef ClientOrderId _spawn_client_order_id(self, Order primary):
        cdef int spawn_sequence = self._exec_spawn_ids.get(primary.client_order_id, 0)
        spawn_sequence += 1
        self._exec_spawn_ids[primary.client_order_id] = spawn_sequence

        return ClientOrderId(f"{primary.client_order_id.to_str()}-E{spawn_sequence}")

    cdef void _reduce_primary_order(self, Order primary, Quantity spawn_qty):
        cdef uint8_t size_precision = primary.quantity._mem.precision
        cdef uint64_t new_raw = primary.quantity._mem.raw - spawn_qty._mem.raw
        cdef Quantity new_qty = Quantity.from_raw_c(new_raw, size_precision)

        # Generate event
        cdef uint64_t now = self._clock.timestamp_ns()

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
            ts_event=now,
            ts_init=now,
        )

        primary.apply(updated)
        self.cache.update_order(primary)

        # Publish updated event
        self._msgbus.publish_c(
            topic=f"events.order.{primary.strategy_id.to_str()}",
            msg=updated,
        )

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
            If `command.exec_algorithm_id` is not equal to `self.id`

        """
        Condition.not_none(command, "command")
        Condition.equal(command.exec_algorithm_id, self.id, "command.exec_algorithm_id", "self.id")

        self._log.debug(f"{RECV}{CMD} {command}.", LogColor.MAGENTA)

        cdef Order order
        if isinstance(command, SubmitOrder):
            self.on_order(command.order)
        elif isinstance(command, SubmitOrderList):
            for order in command.order_list:
                Condition.equal(order.exec_algorithm_id, self.id, "order.exec_algorithm_id", "self.id")
            self.on_order_list(command.order_list)
        else:
            self._log.error(f"Cannot handle command: unrecognized {command}.")

# -- EVENT HANDLERS -------------------------------------------------------------------------------

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
        pass  # Optionally override in subclass

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
        pass

# -- TRADING COMMANDS -----------------------------------------------------------------------------

    cpdef MarketOrder spawn_market(
        self,
        Order primary,
        Quantity quantity,
        TimeInForce time_in_force = TimeInForce.GTC,
        bint reduce_only = False,
        str tags = None,
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
        tags : str, optional
            The custom user tags for the order. These are optional and can
            contain any arbitrary delimiter if required.

        Returns
        -------
        MarketOrder

        Raises
        ------
        ValueError
            If `primary.status` is not ``INITIALIZED``.
        ValueError
            If `primary.exec_algorithm_id` is not equal to `self.id`.
        ValueError
            If `quantity` is not positive (> 0) or not less than `primary.quantity`.
        ValueError
            If `time_in_force` is ``GTD``.

        """
        Condition.not_none(primary, "primary")
        Condition.not_none(quantity, "quantity")
        Condition.equal(primary.status, OrderStatus.INITIALIZED, "primary.status", "order_status")
        Condition.equal(primary.exec_algorithm_id, self.id, "primary.exec_algorithm_id", "id")
        Condition.true(quantity < primary.quantity, "spawning order quantity was not less than `primary.quantity`")

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
        str tags = None,
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
            The spawned orders emulation trigger.
        tags : str, optional
            The custom user tags for the order. These are optional and can
            contain any arbitrary delimiter if required.

        Returns
        -------
        LimitOrder

        Raises
        ------
        ValueError
            If `primary.status` is not ``INITIALIZED``.
        ValueError
            If `primary.exec_algorithm_id` is not equal to `self.id`.
        ValueError
            If `quantity` is not positive (> 0) or not less than `primary.quantity`.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        Condition.not_none(primary, "primary")
        Condition.not_none(quantity, "quantity")
        Condition.equal(primary.status, OrderStatus.INITIALIZED, "primary.status", "order_status")
        Condition.equal(primary.exec_algorithm_id, self.id, "primary.exec_algorithm_id", "id")
        Condition.true(quantity < primary.quantity, "spawning order quantity was not less than `primary.quantity`")

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
        str tags = None,
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
            The spawned orders emulation trigger.
        tags : str, optional
            The custom user tags for the order. These are optional and can
            contain any arbitrary delimiter if required.

        Returns
        -------
        MarketToLimitOrder

        Raises
        ------
        ValueError
            If `primary.status` is not ``INITIALIZED``.
        ValueError
            If `primary.exec_algorithm_id` is not equal to `self.id`.
        ValueError
            If `quantity` is not positive (> 0) or not less than `primary.quantity`.
        ValueError
            If `time_in_force` is ``GTD`` and `expire_time` <= UNIX epoch.
        ValueError
            If `display_qty` is negative (< 0) or greater than `quantity`.

        """
        Condition.not_none(primary, "primary")
        Condition.not_none(quantity, "quantity")
        Condition.equal(primary.status, OrderStatus.INITIALIZED, "primary.status", "order_status")
        Condition.equal(primary.exec_algorithm_id, self.id, "primary.exec_algorithm_id", "id")
        Condition.true(quantity < primary.quantity, "spawning order quantity was not less than `primary.quantity`")

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
            If `order.status` is not ``INITIALIZED``.

        Warning
        -------
        If a `position_id` is passed and a position does not yet exist, then any
        position opened by the order will have this position ID assigned. This may
        not be what you intended.

        """
        Condition.true(self.trader_id is not None, "The execution algorithm has not been registered")
        Condition.not_none(order, "order")
        Condition.equal(order.status, OrderStatus.INITIALIZED, "order", "order_status")

        cdef SubmitOrder primary_command = None
        cdef SubmitOrder spawned_command = None

        if order.exec_spawn_id is not None:
            # Handle new spawned order
            primary_command = self.cache.load_submit_order_command(order.exec_spawn_id)
            Condition.equal(order.strategy_id, primary_command.strategy_id, "order.strategy_id", "primary_command.strategy_id")
            if primary_command is None:
                self._log.error(
                    "Cannot submit order: cannot find primary "
                    f"`SubmitOrder` command for {repr(order.exec_spawn_id)}."
                )
                return

            if self.cache.order_exists(order.client_order_id):
                self._log.error(
                    f"Cannot submit order: order already exists for {repr(order.client_order_id)}.",
                )
                return

            # Publish initialized event
            self._msgbus.publish_c(
                topic=f"events.order.{order.strategy_id.to_str()}",
                msg=order.init_event_c(),
            )

            self.cache.add_order(order, primary_command.position_id)

            spawned_command = SubmitOrder(
                trader_id=self.trader_id,
                strategy_id=primary_command.strategy_id,
                order=order,
                command_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
                position_id=primary_command.position_id,
                client_id=primary_command.client_id,
            )
            self.cache.add_submit_order_command(spawned_command)

            self._send_risk_command(spawned_command)
            return

        # Handle primary (original) order
        primary_command = self.cache.load_submit_order_command(order.client_order_id)
        Condition.equal(order.strategy_id, primary_command.strategy_id, "order.strategy_id", "primary_command.strategy_id")
        if primary_command is None:
            self._log.error(
                "Cannot submit order: cannot find primary "
                f"`SubmitOrder` command for {repr(order.client_order_id)}."
            )
            return
        self._send_risk_command(primary_command)

# -- EGRESS ---------------------------------------------------------------------------------------

    cdef void _send_risk_command(self, TradingCommand command):
        if not self.log.is_bypassed:
            self.log.info(f"{CMD}{SENT} {command}.")
        self._msgbus.send(endpoint="RiskEngine.execute", msg=command)
