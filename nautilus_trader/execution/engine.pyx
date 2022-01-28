# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
The `ExecutionEngine` is the central component of the entire execution stack.

The execution engines primary responsibility is to orchestrate interactions
between the `ExecutionClient` instances, and the rest of the platform. This
includes sending commands to, and receiving events from, the trading venue
endpoints via its registered execution clients.

The engine employs a simple fan-in fan-out messaging pattern to execute
`TradingCommand` messages, and process `AccountState` or `OrderEvent` type
messages.

Alternative implementations can be written on top of the generic engine - which
just need to override the `execute` and `process` methods.
"""

from decimal import Decimal
from typing import Optional

from nautilus_trader.execution.config import ExecEngineConfig

from libc.stdint cimport int64_t

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.generators cimport PositionIdGenerator
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport RPT
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.core.time cimport unix_timestamp_ms
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.reports cimport ExecutionMassStatus
from nautilus_trader.execution.reports cimport ExecutionReport
from nautilus_trader.execution.reports cimport OrderStatusReport
from nautilus_trader.execution.reports cimport PositionStatusReport
from nautilus_trader.execution.reports cimport TradeReport
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.c_enums.oms_type cimport OMSTypeParser
from nautilus_trader.model.c_enums.order_status cimport OrderStatus
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.trailing_offset_type cimport TrailingOffsetTypeParser
from nautilus_trader.model.c_enums.trigger_type cimport TriggerTypeParser
from nautilus_trader.model.commands.trading cimport CancelAllOrders
from nautilus_trader.model.commands.trading cimport CancelOrder
from nautilus_trader.model.commands.trading cimport ModifyOrder
from nautilus_trader.model.commands.trading cimport SubmitOrder
from nautilus_trader.model.commands.trading cimport SubmitOrderList
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.events.position cimport PositionChanged
from nautilus_trader.model.events.position cimport PositionClosed
from nautilus_trader.model.events.position cimport PositionEvent
from nautilus_trader.model.events.position cimport PositionOpened
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.unpacker cimport OrderUnpacker
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class ExecutionEngine(Component):
    """
    Provides a high-performance execution engine for the management of many
    `ExecutionClient` instances, and the asynchronous ingest and distribution of
    trading commands and events.

    Parameters
    ----------
    msgbus : MessageBus
        The message bus for the engine.
    cache : Cache
        The cache for the engine.
    clock : Clock
        The clock for the engine.
    logger : Logger
        The logger for the engine.
    config : ExecEngineConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `ExecEngineConfig`.
    """

    def __init__(
        self,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        config: Optional[ExecEngineConfig]=None,
    ):
        if config is None:
            config = ExecEngineConfig()
        Condition.type(config, ExecEngineConfig, "config")
        super().__init__(
            clock=clock,
            logger=logger,
            component_id=ComponentId("ExecEngine"),
            msgbus=msgbus,
            config=config.dict(),
        )

        self._cache = cache

        self._clients = {}           # type: dict[ClientId, ExecutionClient]
        self._routing_map = {}       # type: dict[Venue, ExecutionClient]
        self._oms_types = {}         # type: dict[StrategyId, OMSType]
        self._default_client = None  # type: Optional[ExecutionClient]

        self._pos_id_generator = PositionIdGenerator(
            trader_id=msgbus.trader_id,
            clock=clock,
        )

        # Counters
        self.command_count = 0
        self.event_count = 0
        self.report_count = 0

        # Register endpoints
        self._msgbus.register(endpoint="ExecEngine.execute", handler=self.execute)
        self._msgbus.register(endpoint="ExecEngine.process", handler=self.process)
        self._msgbus.register(endpoint="ExecEngine.reconcile_report", handler=self.reconcile_report)

    @property
    def registered_clients(self):
        """
        The execution clients registered with the engine.

        Returns
        -------
        list[ClientId]

        """
        return sorted(list(self._clients.keys()))

    @property
    def default_client(self):
        """
        The default execution client registered with the engine.

        Returns
        -------
        Optional[ClientId]

        """
        return self._default_client.id if self._default_client is not None else None

    cpdef int position_id_count(self, StrategyId strategy_id) except *:
        """
        The position ID count for the given strategy ID.

        Parameters
        ----------
        strategy_id : StrategyId
            The strategy ID for the position count.

        Returns
        -------
        int

        """
        return self._pos_id_generator.get_count(strategy_id)

    cpdef bint check_integrity(self) except *:
        """
        Check integrity of data within the cache and clients.

        Returns
        -------
        bool
            True if checks pass, else False.
        """
        return self._cache.check_integrity()

    cpdef bint check_connected(self) except *:
        """
        Check all of the engines clients are connected.

        Returns
        -------
        bool
            True if all clients connected, else False.

        """
        cdef ExecutionClient client
        for client in self._clients.values():
            if not client.is_connected:
                return False
        return True

    cpdef bint check_disconnected(self) except *:
        """
        Check all of the engines clients are disconnected.

        Returns
        -------
        bool
            True if all clients disconnected, else False.

        """
        cdef ExecutionClient client
        for client in self._clients.values():
            if client.is_connected:
                return False
        return True

    cpdef bint check_residuals(self) except *:
        """
        Check for any residual active state and log warnings if found.

        Active state is considered working orders and open positions.

        Returns
        -------
        bool
            True if residuals exist, else False.

        """
        return self._cache.check_residuals()

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_client(self, ExecutionClient client) except *:
        """
        Register the given execution client with the execution engine.

        If the `client.venue` is ``None`` and a default routing client has not
        been previously registered then will be registered as such.

        Parameters
        ----------
        client : ExecutionClient
            The execution client to register.

        Raises
        ------
        ValueError
            If `client` is already registered with the execution engine.

        """
        Condition.not_none(client, "client")
        Condition.not_in(client.id, self._clients, "client.id", "self._clients")

        self._clients[client.id] = client

        routing_log = ""
        if client.venue is None:
            if self._default_client is None:
                self._default_client = client
                routing_log = " for default routing"
        else:
            self._routing_map[client.venue] = client

        self._log.info(f"Registered ExecutionClient-{client}{routing_log}.")

    cpdef void register_default_client(self, ExecutionClient client) except *:
        """
        Register the given client as the default routing client (when a specific
        venue routing cannot be found).

        Any existing default routing client will be overwritten.

        Parameters
        ----------
        client : ExecutionClient
            The client to register.

        """
        Condition.not_none(client, "client")

        self._default_client = client

        self._log.info(f"Registered ExecutionClient-{client} for default routing.")

    cpdef void register_venue_routing(self, ExecutionClient client, Venue venue) except *:
        """
        Register the given client to route orders to the given venue.

        Any existing client in the routing map for the given venue will be
        overwritten.

        Parameters
        ----------
        venue : Venue
            The venue to route orders to.
        client : ExecutionClient
            The client for the venue routing.

        """
        Condition.not_none(client, "client")
        Condition.not_none(venue, "venue")

        if client.id not in self._clients:
            self._clients[client.id] = client

        self._routing_map[venue] = client

        self._log.info(f"Registered ExecutionClient-{client} for routing to {venue}.")

    cpdef void register_oms_type(self, TradingStrategy strategy) except *:
        """
        Register the given trading strategies OMS (Order Management System) type.

        Parameters
        ----------
        strategy : TradingStrategy
            The strategy for the registration.

        """
        Condition.not_none(strategy, "strategy")

        self._oms_types[strategy.id] = strategy.oms_type

        self._log.info(
            f"Registered OMS.{OMSTypeParser.to_str(strategy.oms_type)} "
            f"for TradingStrategy {strategy}.",
        )

    cpdef void deregister_client(self, ExecutionClient client) except *:
        """
        Deregister the given execution client from the execution engine.

        Parameters
        ----------
        client : ExecutionClient
            The execution client to deregister.

        Raises
        ------
        ValueError
            If `client` is not registered with the execution engine.

        """
        Condition.not_none(client, "client")
        Condition.is_in(client.id, self._clients, "client.id", "self._clients")

        del self._clients[client.id]

        if client.venue is None:
            if self._default_client == client:
                self._default_client = None
        else:
            del self._routing_map[client.venue]

        self._log.info(f"Deregistered {client}.")

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _on_start(self) except *:
        pass  # Optionally override in subclass

    cpdef void _on_stop(self) except *:
        pass  # Optionally override in subclass

# -- ACTION IMPLEMENTATIONS ------------------------------------------------------------------------

    cpdef void _start(self) except *:
        cdef ExecutionClient client
        for client in self._clients.values():
            client.start()

        self._on_start()

    cpdef void _stop(self) except *:
        cdef ExecutionClient client
        for client in self._clients.values():
            client.stop()

        self._on_stop()

    cpdef void _reset(self) except *:
        for client in self._clients.values():
            client.reset()

        self._cache.reset()
        self._pos_id_generator.reset()

        self.command_count = 0
        self.event_count = 0
        self.report_count = 0

    cpdef void _dispose(self) except *:
        cdef ExecutionClient client
        for client in self._clients.values():
            client.dispose()

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void load_cache(self) except *:
        """
        Load the cache up from the execution database.
        """
        cdef int64_t ts = unix_timestamp_ms()

        self._cache.cache_currencies()
        self._cache.cache_instruments()
        self._cache.cache_accounts()
        self._cache.cache_orders()
        self._cache.cache_positions()
        self._cache.build_index()
        self._cache.check_integrity()
        self._set_position_id_counts()

        self._log.info(f"Loaded cache in {(unix_timestamp_ms() - ts)}ms.")

    cpdef void execute(self, TradingCommand command) except *:
        """
        Execute the given command.

        Parameters
        ----------
        command : TradingCommand
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._execute_command(command)

    cpdef void process(self, OrderEvent event) except *:
        """
        Process the given order event.

        Parameters
        ----------
        event : OrderEvent
            The order event to process.

        """
        Condition.not_none(event, "event")

        self._handle_event(event)

    cpdef void reconcile_report(self, ExecutionReport report) except *:
        """
        Check the given execution report.

        Parameters
        ----------
        report : Document
            The execution report to check.

        """
        Condition.not_none(report, "report")

        self._reconcile_report(report)

    cpdef void reconcile_mass_status(self, ExecutionMassStatus report) except *:
        """
        Reconcile the given execution mass status report.

        Parameters
        ----------
        report : Document
            The execution mass status report to reconcile.

        """
        Condition.not_none(report, "report")

        self._reconcile_mass_status(report)

    cpdef void flush_db(self) except *:
        """
        Flush the execution database which permanently removes all persisted data.

        Warnings
        --------
        Permanent data loss.

        """
        self._cache.flush_db()

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef void _set_position_id_counts(self) except *:
        # For the internal position ID generator
        cdef list positions = self._cache.positions()

        # Count positions per instrument_id
        cdef dict counts = {}  # type: dict[StrategyId, int]
        cdef int count
        cdef Position position
        for position in positions:
            count = counts.get(position.strategy_id, 0)
            count += 1
            counts[position.strategy_id] = count

        # Reset position ID generator
        self._pos_id_generator.reset()

        # Set counts
        cdef StrategyId strategy_id
        for strategy_id, count in counts.items():
            self._pos_id_generator.set_count(strategy_id, count)
            self._log.info(f"Set PositionId count for {repr(strategy_id)} to {count}.")

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef void _execute_command(self, TradingCommand command) except *:
        self._log.debug(f"{RECV}{CMD} {command}.")
        self.command_count += 1

        cdef ExecutionClient client = self._routing_map.get(
            command.instrument_id.venue,
            self._default_client,
        )
        if client is None:
            self._log.error(
                f"Cannot execute command: "
                f"No execution client configured for {command.instrument_id}, {command}."
            )
            return  # No client to handle command

        if isinstance(command, SubmitOrder):
            self._handle_submit_order(client, command)
        elif isinstance(command, SubmitOrderList):
            self._handle_submit_order_list(client, command)
        elif isinstance(command, ModifyOrder):
            self._handle_modify_order(client, command)
        elif isinstance(command, CancelOrder):
            self._handle_cancel_order(client, command)
        elif isinstance(command, CancelAllOrders):
            self._handle_cancel_all_orders(client, command)
        else:  # pragma: no cover (design-time error)
            self._log.error(f"Cannot handle command: unrecognized {command}.")

    cdef void _handle_submit_order(self, ExecutionClient client, SubmitOrder command) except *:
        # Cache order
        self._cache.add_order(command.order, command.position_id)

        # Send to execution client
        client.submit_order(command)

    cdef void _handle_submit_order_list(self, ExecutionClient client, SubmitOrderList command) except *:
        # Cache all orders
        cdef Order order
        for order in command.list.orders:
            self._cache.add_order(order, position_id=None)

        # Send to execution client
        client.submit_order_list(command)

    cdef void _handle_modify_order(self, ExecutionClient client, ModifyOrder command) except *:
        client.modify_order(command)

    cdef void _handle_cancel_order(self, ExecutionClient client, CancelOrder command) except *:
        client.cancel_order(command)

    cdef void _handle_cancel_all_orders(self, ExecutionClient client, CancelAllOrders command) except *:
        client.cancel_all_orders(command)

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, OrderEvent event) except *:
        self._log.debug(f"{RECV}{EVT} {event}.")
        self.event_count += 1

        # Fetch Order from cache
        cdef ClientOrderId client_order_id = event.client_order_id
        cdef Order order = self._cache.order(event.client_order_id)
        if order is None:
            self._log.warning(
                f"Order with {repr(event.client_order_id)} "
                f"not found in the cache to apply {event}."
            )

            # Search cache for ClientOrderId matching the VenueOrderId
            client_order_id = self._cache.client_order_id(event.venue_order_id)
            if client_order_id is None:
                self._log.error(
                    f"Cannot apply event to any order: "
                    f"{repr(event.client_order_id)} and {repr(event.venue_order_id)} "
                    f"not found in the cache."
                )
                return  # Cannot process event further

            # Search cache for Order matching the found ClientOrderId
            order = self._cache.order(client_order_id)
            if order is None:
                self._log.error(
                    f"Cannot apply event to any order: "
                    f"{repr(event.client_order_id)} and {repr(event.venue_order_id)} "
                    f"not found in the cache."
                )
                return  # Cannot process event further

            # Set the correct ClientOrderId for the event
            event.client_order_id = client_order_id
            self._log.info(
                f"Order with {repr(client_order_id)} was found in the cache.",
                color=LogColor.GREEN,
            )

        # Confirm OMS for strategy
        cdef OMSType oms_type = self._confirm_oms_type(
            event.instrument_id.venue,
            event.strategy_id,
        )

        if isinstance(event, OrderFilled):
            self._confirm_position_id(event, oms_type)

        try:
            order.apply(event)
        except InvalidStateTrigger as ex:
            self._log.warning(f"InvalidStateTrigger: {ex}, did not apply {event}")
            return
        except ValueError as ex:
            # Protection against invalid IDs
            self._log.error(str(ex))
            return
        except KeyError as ex:
            # Protection against duplicate fills
            self._log.error(str(ex))
            return

        self._cache.update_order(order)
        self._msgbus.publish_c(
            topic=f"events.order.{event.strategy_id.value}",
            msg=event,
        )

        if isinstance(event, OrderFilled):
            self._handle_order_fill(event, oms_type)

    cdef OMSType _confirm_oms_type(self, Venue venue, StrategyId strategy_id) except *:
        cdef:
            OMSType oms_type
            ExecutionClient client

        oms_type = self._oms_types.get(strategy_id, 0)
        if oms_type == 0:
            # No OMS configured - use venue OMS
            client = self._clients.get(venue)
            if client is None:
                oms_type = OMSType.HEDGING
            else:
                oms_type = OMSType.HEDGING  # TODO(cs): Set default venue OMS
            # Set OMS for strategy
            self._oms_types[strategy_id] = oms_type

        return oms_type

    cdef void _confirm_position_id(self, OrderFilled fill, OMSType oms_type) except *:
        # Fetch ID from cache
        cdef PositionId position_id = self._cache.position_id(fill.client_order_id)
        if position_id is not None:
            if fill.position_id is not None and fill.position_id != position_id:
                self._log.error(
                    "Incorrect position ID assigned to fill: "
                    f"cached={repr(position_id)}, assigned={repr(fill.position_id)}. "
                    "re-assigning from cache.",
                )
            # Assign position ID to fill
            fill.position_id = position_id
            return

        if oms_type == OMSType.HEDGING:
            if fill.position_id is not None:
                # Already assigned
                return
            # Assign new position ID
            fill.position_id = self._pos_id_generator.generate(fill.strategy_id)
        elif oms_type == OMSType.NETTING:
            # Assign netted position ID singleton
            fill.position_id = PositionId(f"{fill.instrument_id.value}-{fill.strategy_id.value}")
        else:  # pragma: no cover
            raise ValueError(f"invalid OMSType, was {oms_type}")

    cdef void _handle_order_fill(self, OrderFilled fill, OMSType oms_type) except *:
        cdef Position position = self._cache.position(fill.position_id)
        if position is None:
            self._open_position(fill, oms_type)
        else:
            self._update_position(position, fill, oms_type)

    cdef void _open_position(self, OrderFilled fill, OMSType oms_type) except *:
        cdef Instrument instrument = self._cache.load_instrument(fill.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot open position: "
                f"no instrument found for {fill.instrument_id.value}, {fill}."
            )
            return

        cdef Position position = Position(instrument, fill)
        self._cache.add_position(position, oms_type)

        cdef PositionOpened event = PositionOpened.create_c(
            position=position,
            fill=fill,
            event_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.publish_c(
            topic=f"events.position.{event.strategy_id.value}",
            msg=event,
        )

    cdef void _update_position(self, Position position, OrderFilled fill, OMSType oms_type) except *:
        # Check for flip (last_qty guaranteed to be positive)
        if (
            oms_type == OMSType.HEDGING
            and position.is_opposite_side(fill.order_side)
            and fill.last_qty > position.quantity
        ):
            self._flip_position(position, fill, oms_type)
            return  # Handled in flip

        try:
            # Protected against duplicate OrderFilled
            position.apply(fill)
        except KeyError as ex:
            self._log.exception(ex)
            return  # Not re-raising to avoid crashing engine

        self._cache.update_position(position)

        cdef PositionEvent position_event
        if position.is_closed_c():
            event = PositionClosed.create_c(
                position=position,
                fill=fill,
                event_id=self._uuid_factory.generate(),
                ts_init=self._clock.timestamp_ns(),
            )
        else:
            event = PositionChanged.create_c(
                position=position,
                fill=fill,
                event_id=self._uuid_factory.generate(),
                ts_init=self._clock.timestamp_ns(),
            )

        self._msgbus.publish_c(
            topic=f"events.position.{event.strategy_id.value}",
            msg=event,
        )

    cdef void _flip_position(self, Position position, OrderFilled fill, OMSType oms_type) except *:
        cdef Quantity difference = None
        if position.side == PositionSide.LONG:
            difference = Quantity(fill.last_qty - position.quantity, position.size_precision)
        elif position.side == PositionSide.SHORT:
            difference = Quantity(abs(position.quantity - fill.last_qty), position.size_precision)
        else:
            difference = fill.last_qty

        # Split commission between two positions
        fill_percent: Decimal = position.quantity / fill.last_qty
        cdef Money commission1 = Money(fill.commission * fill_percent, fill.commission.currency)
        cdef Money commission2 = Money(fill.commission - commission1, fill.commission.currency)

        cdef OrderFilled fill_split1 = None
        if position.is_open_c():
            # Split fill to close original position
            fill_split1 = OrderFilled(
                trader_id=fill.trader_id,
                strategy_id=fill.strategy_id,
                account_id=fill.account_id,
                instrument_id=fill.instrument_id,
                client_order_id=fill.client_order_id,
                venue_order_id=fill.venue_order_id,
                trade_id=fill.trade_id,
                position_id=fill.position_id,
                order_side=fill.order_side,
                order_type=fill.order_type,
                last_qty=position.quantity,  # Fill original position quantity remaining
                last_px=fill.last_px,
                currency=fill.currency,
                commission=commission1,
                liquidity_side=fill.liquidity_side,
                event_id=fill.id,
                ts_event=fill.ts_event,
                ts_init=fill.ts_init,
            )

            # Close original position
            self._update_position(position, fill_split1, oms_type)

        cdef PositionId position_id_flip = fill.position_id
        if oms_type == OMSType.HEDGING:
            # Generate new position ID for flipped position
            position_id_flip = self._pos_id_generator.generate(
                strategy_id=fill.strategy_id,
                flipped=True,
            )

        # Generate order fill for flipped position
        cdef OrderFilled fill_split2 = OrderFilled(
            trader_id=fill.trader_id,
            strategy_id=fill.strategy_id,
            account_id=fill.account_id,
            instrument_id=fill.instrument_id,
            client_order_id=fill.client_order_id,
            venue_order_id=fill.venue_order_id,
            trade_id=fill.trade_id,
            position_id=position_id_flip,
            order_side=fill.order_side,
            order_type=fill.order_type,
            last_qty=difference,  # Fill difference from original as above
            last_px=fill.last_px,
            currency=fill.currency,
            commission=commission2,
            liquidity_side=fill.liquidity_side,
            event_id=self._uuid_factory.generate(),  # New event ID
            ts_event=fill.ts_event,
            ts_init=fill.ts_init,
        )

        # Open flipped position
        self._handle_order_fill(fill_split2, oms_type)

# -- RECONCILIATION --------------------------------------------------------------------------------

    cdef bint _reconcile_report(self, ExecutionReport report) except *:
        self._log.debug(f"{RECV}{RPT} {report}.")
        self.report_count += 1

        if isinstance(report, OrderStatusReport):
            self._reconcile_order(report, [])  # No trades to reconcile
        elif isinstance(report, TradeReport):
            pass  # TODO: Implement
        elif isinstance(report, PositionStatusReport):
            pass  # TODO: Implement
        else:  # pragma: no cover (design-time error)
            self._log.error(f"Cannot handle report: unrecognized {report}.")

        self._msgbus.publish_c(
            topic=f"reports.execution"
                  f".{report.instrument_id.venue}"
                  f".{report.instrument_id.symbol}",
            msg=report,
        )

        return True  # TODO(cs): Implement

    cdef bint _reconcile_mass_status(self, ExecutionMassStatus mass_status) except *:
        self._log.debug(f"{RECV}{RPT} {mass_status}.")
        self.report_count += 1

        cdef dict trade_reports = mass_status.trade_reports()

        cdef list results = []

        # Reconcile all reported orders
        for venue_order_id, order_report in mass_status.order_reports().items():
            trades = trade_reports.get(venue_order_id, [])
            result = self._reconcile_order(order_report, trades)
            results.append(result)

        # TODO(cs): Check all reported positions

        # Publish mass status
        self._msgbus.publish_c(
            topic=f"reports.execution.{mass_status.venue.value}",
            msg=mass_status,
        )

        return all(results)

    cdef bint _reconcile_order(self, OrderStatusReport report, list trades) except *:
        cdef ClientOrderId client_order_id = report.client_order_id
        if client_order_id is None:
            client_order_id = self._cache.client_order_id(report.venue_order_id)
            if client_order_id is None:
                # Generate external client order ID
                client_order_id = self._generate_client_order_id()
            # Assign to report
            report.client_order_id = client_order_id

        cdef Order order = self._cache.order(client_order_id)
        if order is None:
            order = self._generate_external_order(report)
            # Add to cache without determining any position ID initially
            self._cache.add_order(order, position_id=None)

        if report.order_status == OrderStatus.REJECTED:
            if order.status_c() != OrderStatus.REJECTED:
                self._apply_order_rejected(order, report)
            return True  # Reconciled

        if report.order_status == OrderStatus.ACCEPTED:
            if order.status_c() != OrderStatus.ACCEPTED:
                self._apply_order_accepted(order, report)
            return True  # Reconciled

        if order.status_c() == OrderStatus.INITIALIZED or order.status_c() == OrderStatus.SUBMITTED:
            self._apply_order_accepted(order, report)

        # Update order if necessary
        if self._should_update(order, report):
            self._apply_order_updated(order, report)

        if report.order_status == OrderStatus.TRIGGERED:
            if order.status_c() in (OrderStatus.INITIALIZED or OrderStatus.SUBMITTED):
                self._apply_order_accepted(order, report)
            if order.status_c() != OrderStatus.TRIGGERED:
                self._apply_order_triggered(order, report)
            return True  # Reconciled

        if report.order_status == OrderStatus.CANCELED:
            if order.status_c() == OrderStatus.INITIALIZED or order.status_c() == OrderStatus.SUBMITTED:
                self._apply_order_accepted(order, report)
            if order.status_c() != OrderStatus.CANCELED:
                if report.ts_triggered > 0:
                    self._apply_order_triggered(order, report)
                self._apply_order_canceled(order, report)
            return True  # Reconciled

        if report.order_status == OrderStatus.EXPIRED:
            if order.status_c() == OrderStatus.INITIALIZED or order.status_c() == OrderStatus.SUBMITTED:
                self._apply_order_accepted(order, report)
            if order.status_c() != OrderStatus.EXPIRED:
                if report.ts_triggered > 0:
                    self._apply_order_triggered(order, report)
                self._apply_order_expired(order, report)
            return True  # Reconciled

        cdef Instrument instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot reconcile order {order.client_order_id}: "
                f"instrument {order.instrument_id} not found.",
            )
            return False  # Failed

        cdef:
            TradeReport trade
            OrderFilled fill
        for trade in trades:
            if trade.trade_id in order.trade_ids_c():
                continue  # Fill already applied
            self._apply_order_filled(order, trade, instrument)

        # if report.filled_qty != order.filled_qty:
        #     self._log.error(
        #         f"Cannot reconcile order {order.client_order_id}: "
        #         f"reported filled qty {report.filled_qty} != order.filled_qty {order.filled_qty}.",
        #     )
        #     return False  # Failed

        return True  # Reconciled

    cdef ClientOrderId _generate_client_order_id(self):
        return ClientOrderId(f"O-{self._uuid_factory.generate().value}")

    cdef Order _generate_external_order(self, OrderStatusReport report):
        # Prepare order options
        cdef dict options = {}
        if report.price is not None:
            options["price"] = str(report.price)
        if report.trigger_price is not None:
            options["trigger_price"] = str(report.trigger_price)
            options["trigger_type"] = TriggerTypeParser.to_str(report.trigger_type)
        if report.limit_offset is not None:
            options["limit_offset"] = str(report.limit_offset)
            options["offset_type"] =  TrailingOffsetTypeParser.to_str(report.offset_type)
        if report.trailing_offset is not None:
            options["trailing_offset"] = str(report.trailing_offset)
        if report.display_qty is not None:
            options["display_qty"] = str(report.display_qty)
        if report.expire_time is not None:
            expire_time_ns: int = dt_to_unix_nanos(report.expire_time)
            if expire_time_ns > 0:
                options["expire_time_ns"] = expire_time_ns

        cdef initialized = OrderInitialized(
            trader_id=self.trader_id,
            strategy_id=StrategyId("EXTERNAL-000"),
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            order_side=report.order_side,
            order_type=report.order_type,
            quantity=report.quantity,
            time_in_force=report.time_in_force,
            post_only=report.post_only,
            reduce_only=report.reduce_only,
            options=options,
            order_list_id=report.order_list_id,
            contingency_type=report.contingency_type,
            linked_order_ids=None,
            parent_order_id=None,
            tags="EXTERNAL",
            event_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        return OrderUnpacker.from_init_c(initialized)

    cdef void _apply_order_rejected(self, Order order, OrderStatusReport report) except *:
        cdef OrderRejected rejected = OrderRejected(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            account_id=report.account_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            reason=report.reject_reason or "UNKNOWN",
            event_id=self._uuid_factory.generate(),
            ts_event=report.ts_last,
            ts_init=self._clock.timestamp_ns(),
        )
        order.apply(rejected)
        self._cache.update_order(order)

    cdef void _apply_order_accepted(self, Order order, OrderStatusReport report) except *:
        cdef OrderAccepted accepted = OrderAccepted(
            trader_id=self.trader_id,
            strategy_id=order.strategy_id,
            account_id=report.account_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            event_id=self._uuid_factory.generate(),
            ts_event=report.ts_accepted,
            ts_init=self._clock.timestamp_ns(),
        )
        order.apply(accepted)
        self._cache.update_order(order)

    cdef void _apply_order_triggered(self, Order order, OrderStatusReport report) except *:
        cdef OrderTriggered triggered = OrderTriggered(
            trader_id=self.trader_id,
            strategy_id=order.strategy_id,
            account_id=report.account_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            event_id=self._uuid_factory.generate(),
            ts_event=report.ts_triggered,
            ts_init=self._clock.timestamp_ns(),
        )
        order.apply(triggered)
        self._cache.update_order(order)

    cdef void _apply_order_updated(self, Order order, OrderStatusReport report) except *:
        cdef OrderUpdated updated = OrderUpdated(
            trader_id=self.trader_id,
            strategy_id=order.strategy_id,
            account_id=report.account_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            quantity=report.quantity,
            price=report.price,
            trigger_price=report.trigger_price,
            event_id=self._uuid_factory.generate(),
            ts_event=report.ts_accepted,
            ts_init=self._clock.timestamp_ns(),
        )
        order.apply(updated)
        self._cache.update_order(order)

    cdef void _apply_order_canceled(self, Order order, OrderStatusReport report) except *:
        cdef OrderCanceled canceled = OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            account_id=report.account_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            event_id=self._uuid_factory.generate(),
            ts_event=report.ts_last,
            ts_init=self._clock.timestamp_ns(),
        )
        order.apply(canceled)
        self._cache.update_order(order)

    cdef void _apply_order_expired(self, Order order, OrderStatusReport report) except *:
        cdef OrderExpired expired = OrderExpired(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            account_id=report.account_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            event_id=self._uuid_factory.generate(),
            ts_event=report.ts_last,
            ts_init=self._clock.timestamp_ns(),
        )
        order.apply(expired)
        self._cache.update_order(order)

    cdef void _apply_order_filled(self, Order order, TradeReport trade, Instrument instrument) except *:
        cdef OrderFilled fill = OrderFilled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            account_id=trade.account_id,
            instrument_id=trade.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=trade.venue_order_id,
            trade_id=trade.trade_id,
            position_id=trade.venue_position_id,
            order_side=order.side,
            order_type=order.type,
            last_qty=trade.last_qty,
            last_px=trade.last_px,
            currency=instrument.quote_currency,
            commission=trade.commission,
            liquidity_side=trade.liquidity_side,
            event_id=self._uuid_factory.generate(),
            ts_event=trade.ts_event,
            ts_init=self._clock.timestamp_ns(),
        )
        order.apply(fill)
        self._cache.update_order(order)

    cdef bint _should_update(self, Order order, OrderStatusReport report) except *:
        if report.quantity != order.quantity:
            return True
        elif order.type == OrderType.LIMIT:
            if report.price != order.price:
                return True
        elif order.type == OrderType.STOP_MARKET or order.type == OrderType.TRAILING_STOP_MARKET:
            if report.trigger_price != order.trigger_price:
                return True
        elif order.type == OrderType.STOP_LIMIT or order.type == OrderType.TRAILING_STOP_LIMIT:
            if report.trigger_price != order.trigger_price or report.price != order.price:
                return True
        return False
