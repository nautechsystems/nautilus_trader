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

from nautilus_trader.config import ExecEngineConfig
from nautilus_trader.config.error import InvalidConfiguration
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import ExecutionReport

from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.generators cimport PositionIdGenerator
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.core.rust.core cimport unix_timestamp_ms
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.algorithm cimport ExecAlgorithm
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.messages cimport BatchCancelOrders
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport ContingencyType
from nautilus_trader.model.enums_c cimport OmsType
from nautilus_trader.model.enums_c cimport PositionSide
from nautilus_trader.model.enums_c cimport oms_type_to_str
from nautilus_trader.model.events.order cimport OrderDenied
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.position cimport PositionChanged
from nautilus_trader.model.events.position cimport PositionClosed
from nautilus_trader.model.events.position cimport PositionEvent
from nautilus_trader.model.events.position cimport PositionOpened
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.currency_pair cimport CurrencyPair
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
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
        config: Optional[ExecEngineConfig] = None,
    ) -> None:
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

        self._cache: Cache = cache

        self._clients: dict[ClientId, ExecutionClient] = {}
        self._routing_map: dict[Venue, ExecutionClient] = {}
        self._default_client: Optional[ExecutionClient] = None
        self._oms_overrides: dict[StrategyId, OmsType] = {}
        self._external_order_claims: dict[InstrumentId, StrategyId] = {}

        self._pos_id_generator: PositionIdGenerator = PositionIdGenerator(
            trader_id=msgbus.trader_id,
            clock=clock,
        )

        # Settings
        self.debug: bool = config.debug
        self.allow_cash_positions: bool = config.allow_cash_positions

        # Counters
        self.command_count: int = 0
        self.event_count: int = 0
        self.report_count: int = 0

        # Register endpoints
        self._msgbus.register(endpoint="ExecEngine.execute", handler=self.execute)
        self._msgbus.register(endpoint="ExecEngine.process", handler=self.process)

    @property
    def reconciliation(self) -> bool:
        """
        Return whether the reconciliation process will be run on start.

        Returns
        -------
        bool

        """
        # Temporary to push down common logic, the `LiveExecutionEngine` will override this
        return False

    @property
    def registered_clients(self) -> list[ClientId]:
        """
        Return the execution clients registered with the engine.

        Returns
        -------
        list[ClientId]

        """
        return sorted(list(self._clients.keys()))

    @property
    def default_client(self) -> Optional[ClientId]:
        """
        Return the default execution client registered with the engine.

        Returns
        -------
        ClientId or ``None``

        """
        return self._default_client.id if self._default_client is not None else None

    def connect(self) -> None:
        """
        Connect the engine by calling connect on all registered clients.
        """
        self._log.info("Connecting all clients...")
        # Implement actual client connections for a live/sandbox context

    def disconnect(self) -> None:
        """
        Disconnect the engine by calling disconnect on all registered clients.
        """
        self._log.info("Disconnecting all clients...")
        # Implement actual client connections for a live/sandbox context

    cpdef int position_id_count(self, StrategyId strategy_id):
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

    cpdef bint check_integrity(self):
        """
        Check integrity of data within the cache and clients.

        Returns
        -------
        bool
            True if checks pass, else False.
        """
        return self._cache.check_integrity()

    cpdef bint check_connected(self):
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

    cpdef bint check_disconnected(self):
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

    cpdef bint check_residuals(self):
        """
        Check for any residual open state and log warnings if found.

        'Open state' is considered to be open orders and open positions.

        Returns
        -------
        bool
            True if residuals exist, else False.

        """
        return self._cache.check_residuals()

    cpdef StrategyId get_external_order_claim(self, InstrumentId instrument_id):
        """
        Get any external order claim for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the claim.

        Returns
        -------
        StrategyId or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._external_order_claims.get(instrument_id)

    cpdef set get_external_order_claims_instruments(self):
        """
        Get all external order claims instrument IDs.

        Returns
        -------
        set[InstrumentId]

        """
        return set(self._external_order_claims.keys())

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register_client(self, ExecutionClient client):
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
        Condition.not_in(client.id, self._clients, "client.id", "_clients")

        self._clients[client.id] = client

        routing_log = ""
        if client.venue is None:
            if self._default_client is None:
                self._default_client = client
                routing_log = " for default routing"
        else:
            self._routing_map[client.venue] = client

        self._log.info(f"Registered ExecutionClient-{client}{routing_log}.")

    cpdef void register_default_client(self, ExecutionClient client):
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

        self._log.info(f"Registered {client} for default routing.")

    cpdef void register_venue_routing(self, ExecutionClient client, Venue venue):
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

    cpdef void register_oms_type(self, Strategy strategy):
        """
        Register the given trading strategies OMS (Order Management System) type.

        Parameters
        ----------
        strategy : Strategy
            The strategy for the registration.

        """
        Condition.not_none(strategy, "strategy")

        self._oms_overrides[strategy.id] = strategy.oms_type

        self._log.info(
            f"Registered OMS.{oms_type_to_str(strategy.oms_type)} "
            f"for Strategy {strategy}.",
        )

    cpdef void register_external_order_claims(self, Strategy strategy):
        """
        Register the given strategies external order claim instrument IDs (if any)

        Parameters
        ----------
        strategy : Strategy
            The strategy for the registration.

        Raises
        ------
        InvalidConfiguration
            If a strategy is already registered to claim external orders for an instrument ID.

        """
        Condition.not_none(strategy, "strategy")

        cdef:
            InstrumentId instrument_id
            StrategyId existing
        for instrument_id in strategy.external_order_claims:
            existing = self._external_order_claims.get(instrument_id)
            if existing:
                raise InvalidConfiguration(
                    f"External order claim for {instrument_id} already exists for {existing}",
                )
            # Register strategy to claim external orders for this instrument
            self._external_order_claims[instrument_id] = strategy.id

        if strategy.external_order_claims:
            self._log.info(
                f"Registered external order claims for {strategy}: {strategy.external_order_claims}.",
            )

    cpdef void deregister_client(self, ExecutionClient client):
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

    # -- RECONCILIATION -------------------------------------------------------------------------------

    async def reconcile_state(self, timeout_secs: float = 10.0) -> bool:
        """
        Reconcile the internal execution state with all execution clients (external state).

        Parameters
        ----------
        timeout_secs : double, default 10.0
            The timeout (seconds) for reconciliation to complete.

        Returns
        -------
        bool
            True if states reconcile within timeout, else False.

        Raises
        ------
        ValueError
            If `timeout_secs` is not positive (> 0).

        """
        return True  # Should be overridden for live execution engines

    def reconcile_report(self, report: ExecutionReport) -> bool:
        """
        Check the given execution report.

        Parameters
        ----------
        report : ExecutionReport
            The execution report to check.

        Returns
        -------
        bool
            True if reconciliation successful, else False.

        """
        return True  # Should be overridden for live execution engines

    def reconcile_mass_status(self, report: ExecutionMassStatus) -> None:
        """
        Reconcile the given execution mass status report.

        Parameters
        ----------
        report : ExecutionMassStatus
            The execution mass status report to reconcile.

        """
        # Should be overridden for live execution enginesj

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _on_start(self):
        pass  # Optionally override in subclass

    cpdef void _on_stop(self):
        pass  # Optionally override in subclass

# -- ACTION IMPLEMENTATIONS -----------------------------------------------------------------------

    cpdef void _start(self):
        cdef ExecutionClient client
        for client in self._clients.values():
            client.start()

        self._on_start()

    cpdef void _stop(self):
        cdef ExecutionClient client
        for client in self._clients.values():
            client.stop()

        self._on_stop()

    cpdef void _reset(self):
        for client in self._clients.values():
            client.reset()

        self._cache.reset()
        self._pos_id_generator.reset()

        self.command_count = 0
        self.event_count = 0
        self.report_count = 0

    cpdef void _dispose(self):
        cdef ExecutionClient client
        for client in self._clients.values():
            client.dispose()

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void load_cache(self):
        """
        Load the cache up from the execution database.
        """
        cdef uint64_t ts = unix_timestamp_ms()

        self._cache.cache_general()
        self._cache.cache_currencies()
        self._cache.cache_instruments()
        self._cache.cache_accounts()
        self._cache.cache_orders()
        self._cache.cache_order_lists()
        self._cache.cache_positions()

        self._cache.build_index()
        self._cache.check_integrity()
        self._set_position_id_counts()

        self._log.info(f"Loaded cache in {(unix_timestamp_ms() - ts)}ms.")

    cpdef void execute(self, TradingCommand command):
        """
        Execute the given command.

        Parameters
        ----------
        command : TradingCommand
            The command to execute.

        """
        Condition.not_none(command, "command")

        self._execute_command(command)

    cpdef void process(self, OrderEvent event):
        """
        Process the given order event.

        Parameters
        ----------
        event : OrderEvent
            The order event to process.

        """
        Condition.not_none(event, "event")

        self._handle_event(event)

    cpdef void flush_db(self):
        """
        Flush the execution database which permanently removes all persisted data.

        Warnings
        --------
        Permanent data loss.

        """
        self._cache.flush_db()

# -- INTERNAL -------------------------------------------------------------------------------------

    cpdef void _set_position_id_counts(self):
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
            self._log.info(f"Set PositionId count for {strategy_id!r} to {count}.")

    cpdef Price _last_px_for_conversion(self, InstrumentId instrument_id, OrderSide order_side):
        cdef Price last_px = None
        cdef QuoteTick last_quote = self._cache.quote_tick(instrument_id)
        cdef TradeTick last_trade = self._cache.trade_tick(instrument_id)
        if last_quote is not None:
            last_px = last_quote.ask_price if order_side == OrderSide.BUY else last_quote.bid_price
        else:
            if last_trade is not None:
                last_px = last_trade.price

        return last_px

    cpdef void _set_order_base_qty(self, Order order, Quantity base_qty):
        self._log.info(
            f"Setting {order.instrument_id} order quote quantity {order.quantity} to base quantity {base_qty}.",
        )
        cdef Quantity original_qty = order.quantity
        order.quantity = base_qty
        order.leaves_qty = base_qty
        order.is_quote_quantity = False

        if order.contingency_type != ContingencyType.OTO:
            return

        # Set base quantity for all OTO contingent orders
        cdef ClientOrderId client_order_id
        cdef Order contingent_order
        for client_order_id in order.linked_order_ids or []:
            contingent_order = self._cache.order(client_order_id)
            if contingent_order is None:
                self._log.error(f"Contingency order {client_order_id!r} not found.")
                continue
            if not contingent_order.is_quote_quantity:
                continue  # Already base quantity
            if contingent_order.quantity != original_qty:
                self._log.warning(
                    f"Contingent order quantity {contingent_order.quantity} "
                    f"was not equal to the OTO parent original quantity {original_qty} "
                    f"when setting to base quantity of {base_qty}."
                )
            self._log.info(
                f"Setting {contingent_order.instrument_id} order quote quantity "
                f"{contingent_order.quantity} to base quantity {base_qty}.",
            )
            contingent_order.quantity = base_qty
            contingent_order.leaves_qty = base_qty
            contingent_order.is_quote_quantity = False

    cpdef void _deny_order(self, Order order, str reason):
        # Generate event
        cdef OrderDenied denied = OrderDenied(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            reason=reason,
            event_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        order.apply(denied)

        self._cache.update_order(order)
        self._msgbus.publish_c(
            topic=f"events.order.{order.strategy_id}",
            msg=denied,
        )

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cpdef void _execute_command(self, TradingCommand command):
        if self.debug:
            self._log.debug(f"{RECV}{CMD} {command}.", LogColor.MAGENTA)
        self.command_count += 1

        cdef ExecutionClient client = self._clients.get(command.client_id)
        if client is None:
            client = self._routing_map.get(
                command.instrument_id.venue,
                self._default_client,
            )
            if client is None:
                self._log.error(
                    f"Cannot execute command: "
                    f"no execution client configured for {command.instrument_id.venue} or `client_id` {command.client_id}, "
                    f"{command}."
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
        elif isinstance(command, BatchCancelOrders):
            self._handle_batch_cancel_orders(client, command)
        elif isinstance(command, QueryOrder):
            self._handle_query_order(client, command)
        else:
            self._log.error(  # pragma: no cover (design-time error)
                f"Cannot handle command: unrecognized {command}.",  # pragma: no cover (design-time error)
            )

    cpdef void _handle_submit_order(self, ExecutionClient client, SubmitOrder command):
        cdef Order order = command.order
        if not self._cache.order_exists(order.client_order_id):
            # Cache order
            self._cache.add_order(order, command.position_id, command.client_id)

        cdef Instrument instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot handle submit order: "
                f"no instrument found for {order.instrument_id}, {command}."
            )
            return

        # Check if converting quote quantity
        cdef Price last_px = None
        cdef Quantity base_qty = None
        if not instrument.is_inverse and order.is_quote_quantity:
            last_px = self._last_px_for_conversion(order.instrument_id, order.side)
            if last_px is None:
                self._deny_order(order, f"no-price-to-convert-quote-qty {order.instrument_id}")
                return  # Denied
            base_qty = instrument.calculate_base_quantity(order.quantity, last_px)
            self._set_order_base_qty(order, base_qty)

        # Send to execution client
        client.submit_order(command)

    cpdef void _handle_submit_order_list(self, ExecutionClient client, SubmitOrderList command):
        cdef Order order
        for order in command.order_list.orders:
            if not self._cache.order_exists(order.client_order_id):
                # Cache order
                self._cache.add_order(order, command.position_id, command.client_id)

        cdef Instrument instrument = self._cache.instrument(command.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot handle submit order list: "
                f"no instrument found for {command.instrument_id}, {command}."
            )
            return

        # Check if converting quote quantity
        cdef Price last_px = None
        cdef Quantity quote_qty = None
        cdef Quantity base_qty = None
        if not instrument.is_inverse and command.order_list.first.is_quote_quantity:
            for order in command.order_list.orders:
                if order.is_quote_quantity == False:
                    continue  # Base quantity already set
                if order.quantity != quote_qty:
                    last_px = self._last_px_for_conversion(order.instrument_id, order.side)
                    quote_qty = order.quantity
                if last_px is None:
                    for order in command.order_list.orders:
                        self._deny_order(order, f"no-price-to-convert-quote-qty {order.instrument_id}")
                    return  # Denied
                base_qty = instrument.calculate_base_quantity(quote_qty, last_px)
                self._set_order_base_qty(order, base_qty)

        # Send to execution client
        client.submit_order_list(command)

    cpdef void _handle_modify_order(self, ExecutionClient client, ModifyOrder command):
        client.modify_order(command)

    cpdef void _handle_cancel_order(self, ExecutionClient client, CancelOrder command):
        client.cancel_order(command)

    cpdef void _handle_cancel_all_orders(self, ExecutionClient client, CancelAllOrders command):
        client.cancel_all_orders(command)

    cpdef void _handle_batch_cancel_orders(self, ExecutionClient client, BatchCancelOrders command):
        client.batch_cancel_orders(command)

    cpdef void _handle_query_order(self, ExecutionClient client, QueryOrder command):
        client.query_order(command)

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void _handle_event(self, OrderEvent event):
        if self.debug:
            self._log.debug(f"{RECV}{EVT} {event}.", LogColor.MAGENTA)
        self.event_count += 1

        # Fetch Order from cache
        cdef ClientOrderId client_order_id = event.client_order_id
        cdef Order order = self._cache.order(event.client_order_id)
        if order is None:
            self._log.warning(
                f"Order with {event.client_order_id!r} "
                f"not found in the cache to apply {event}."
            )

            if event.venue_order_id is None:
                self._log.error(
                    f"Cannot apply event to any order: "
                    f"{event.client_order_id!r} not found in the cache "
                    f"with no `VenueOrderId`."
                )
                return  # Cannot process event further

            # Search cache for ClientOrderId matching the VenueOrderId
            client_order_id = self._cache.client_order_id(event.venue_order_id)
            if client_order_id is None:
                self._log.error(
                    f"Cannot apply event to any order: "
                    f"{event.client_order_id!r} and {event.venue_order_id!r} "
                    f"not found in the cache."
                )
                return  # Cannot process event further

            # Search cache for Order matching the found ClientOrderId
            order = self._cache.order(client_order_id)
            if order is None:
                self._log.error(
                    f"Cannot apply event to any order: "
                    f"{event.client_order_id!r} and {event.venue_order_id!r} "
                    f"not found in the cache."
                )
                return  # Cannot process event further

            # Set the correct ClientOrderId for the event
            event.set_client_order_id(client_order_id)
            self._log.info(
                f"Order with {client_order_id!r} was found in the cache.",
                color=LogColor.GREEN,
            )

        cdef OmsType oms_type
        if isinstance(event, OrderFilled):
            oms_type = self._determine_oms_type(event)
            self._determine_position_id(event, oms_type)
            self._apply_event_to_order(order, event)
            self._handle_order_fill(order, event, oms_type)
        else:
            self._apply_event_to_order(order, event)

    cpdef OmsType _determine_oms_type(self, OrderFilled fill):
        cdef ExecutionClient client
        # Check for strategy OMS override
        cdef OmsType oms_type = self._oms_overrides.get(fill.strategy_id, OmsType.UNSPECIFIED)
        if oms_type == OmsType.UNSPECIFIED:
            # Use native venue OMS
            client = self._routing_map.get(fill.instrument_id.venue, self._default_client)
            if client is None:
                return OmsType.NETTING
            else:
                return client.oms_type

        return oms_type

    cpdef void _determine_position_id(self, OrderFilled fill, OmsType oms_type):
        # Fetch ID from cache
        cdef PositionId position_id = self._cache.position_id(fill.client_order_id)
        if self.debug:
            self._log.debug(
                f"Determining position ID for {fill.client_order_id!r}, "
                f"position_id={position_id!r}.",
                LogColor.MAGENTA,
            )
        if position_id is not None:
            if fill.position_id is not None and fill.position_id != position_id:
                self._log.error(
                    "Incorrect position ID assigned to fill: "
                    f"cached={position_id!r}, assigned={fill.position_id!r}. "
                    "re-assigning from cache.",
                )
            # Assign position ID to fill
            fill.position_id = position_id
            if self.debug:
                self._log.debug(f"Assigned {position_id!r} to {fill}.", LogColor.MAGENTA)
            return

        if oms_type == OmsType.HEDGING:
            position_id = self._determine_hedging_position_id(fill)
        elif oms_type == OmsType.NETTING:
            # Assign netted position ID
            position_id = self._determine_netting_position_id(fill)
        else:
            raise ValueError(  # pragma: no cover (design-time error)
                f"invalid `OmsType`, was {oms_type}",  # pragma: no cover (design-time error)
            )

        fill.position_id = position_id

        # TODO(cs): Optimize away the need to fetch order from cache
        cdef Order order = self._cache.order(fill.client_order_id)
        if order is None:
            raise RuntimeError(
                f"Order for {fill.client_order_id!r} not found to determine position ID.",
            )

        # Check execution algorithm position ID
        if order.exec_algorithm_id is None or order.exec_spawn_id is None:
            return

        cdef Order primary = self._cache.order(order.exec_spawn_id)
        assert primary is not None
        if primary.position_id is None:
            primary.position_id = position_id
            self._cache.add_position_id(
                position_id,
                primary.instrument_id.venue,
                primary.client_order_id,
                primary.strategy_id,
            )
            self._log.debug(f"Assigned primary order {position_id!r}.", LogColor.MAGENTA)

    cpdef PositionId _determine_hedging_position_id(self, OrderFilled fill):
        if fill.position_id is not None:
            if self.debug:
                self._log.debug(f"Already had a position ID of: {fill.position_id!r}", LogColor.MAGENTA)
            # Already assigned
            return fill.position_id

        cdef Order order = self._cache.order(fill.client_order_id)
        if order is None:
            raise RuntimeError(
                f"Order for {fill.client_order_id!r} not found to determine position ID.",
            )

        cdef:
            list exec_spawn_orders
            Order spawned_order
        if order.exec_spawn_id is not None:
            exec_spawn_orders = self._cache.orders_for_exec_spawn(order.exec_spawn_id)
            for spawned_order in exec_spawn_orders:
                if spawned_order.position_id is not None:
                    if self.debug:
                        self._log.debug(f"Found spawned {spawned_order.position_id!r} for {fill}.", LogColor.MAGENTA)
                    # Use position ID for execution spawn
                    return spawned_order.position_id

        # Assign new position ID
        position_id = self._pos_id_generator.generate(fill.strategy_id)
        if self.debug:
            self._log.debug(f"Generated {position_id!r} for {fill}.", LogColor.MAGENTA)
        return position_id

    cpdef PositionId _determine_netting_position_id(self, OrderFilled fill):
        return PositionId(f"{fill.instrument_id}-{fill.strategy_id}")

    cpdef void _apply_event_to_order(self, Order order, OrderEvent event):
        try:
            order.apply(event)
        except InvalidStateTrigger as e:
            self._log.warning(f"InvalidStateTrigger: {e}, did not apply {event}")
            return
        except (ValueError, KeyError) as e:
            # ValueError: Protection against invalid IDs
            # KeyError: Protection against duplicate fills
            self._log.exception(f"Error on applying {event!r} to {order!r}", e)
            return

        self._cache.update_order(order)
        self._msgbus.publish_c(
            topic=f"events.order.{event.strategy_id}",
            msg=event,
        )

    cpdef void _handle_order_fill(self, Order order, OrderFilled fill, OmsType oms_type):
        cdef Instrument instrument = self._cache.load_instrument(fill.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot handle order fill: "
                f"no instrument found for {fill.instrument_id}, {fill}."
            )
            return

        cdef Account account = self._cache.account(fill.account_id)
        if account is None:
            self._log.error(
                f"Cannot handle order fill: "
                f"no account found for {fill.instrument_id.venue}, {fill}."
            )
            return

        if not self.allow_cash_positions and isinstance(instrument, CurrencyPair):
            if account.is_unleveraged(instrument.id):
                return  # No spot cash positions

        cdef Position position = self._cache.position(fill.position_id)
        if position is None or position.is_closed_c():
            position = self._open_position(instrument, position, fill, oms_type)
        elif self._will_flip_position(position, fill):
            self._flip_position(instrument, position, fill, oms_type)
        else:
            self._update_position(instrument, position, fill, oms_type)

        cdef:
            ClientOrderId client_order_id
            Order contingent_order
        if order.contingency_type == ContingencyType.OTO and position is not None and position.is_open_c():
            for client_order_id in order.linked_order_ids or []:
                contingent_order = self._cache.order(client_order_id)
                if contingent_order is not None and contingent_order.position_id is None:
                    contingent_order.position_id = position.id
                    self._cache.add_position_id(
                        position.id,
                        contingent_order.instrument_id.venue,
                        contingent_order.client_order_id,
                        contingent_order.strategy_id,
                    )

    cpdef Position _open_position(self, Instrument instrument, Position position, OrderFilled fill, OmsType oms_type):
        if position is None:
            position = Position(instrument, fill)
            self._cache.add_position(position, oms_type)
        else:
            try:
                self._cache.snapshot_position(position)
                position.apply(fill)
                self._cache.update_position(position)
            except KeyError as e:
                # Protected against duplicate OrderFilled
                self._log.exception(f"Error on applying {fill!r} to {position!r}", e)
                return  # Not re-raising to avoid crashing engine

        cdef PositionOpened event = PositionOpened.create_c(
            position=position,
            fill=fill,
            event_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._msgbus.publish_c(
            topic=f"events.position.{event.strategy_id}",
            msg=event,
        )

        return position

    cpdef void _update_position(self, Instrument instrument, Position position, OrderFilled fill, OmsType oms_type):
        try:
            position.apply(fill)
        except KeyError as e:
            # Protected against duplicate OrderFilled
            self._log.exception(f"Error on applying {fill!r} to {position!r}", e)
            return  # Not re-raising to avoid crashing engine

        self._cache.update_position(position)

        cdef PositionEvent event
        if position.is_closed_c():
            event = PositionClosed.create_c(
                position=position,
                fill=fill,
                event_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )
        else:
            event = PositionChanged.create_c(
                position=position,
                fill=fill,
                event_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )

        self._msgbus.publish_c(
            topic=f"events.position.{event.strategy_id}",
            msg=event,
        )

    cpdef bint _will_flip_position(self, Position position, OrderFilled fill):
        return (
            # Check for flip (last_qty guaranteed to be positive)
            position.is_opposite_side(fill.order_side)
            and fill.last_qty._mem.raw > position.quantity._mem.raw
        )

    cpdef void _flip_position(self, Instrument instrument, Position position, OrderFilled fill, OmsType oms_type):
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
                instrument_id=fill.instrument_id,
                client_order_id=fill.client_order_id,
                venue_order_id=fill.venue_order_id,
                account_id=fill.account_id,
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
            self._update_position(instrument, position, fill_split1, oms_type)

        # Guard against flipping a position with a zero fill size
        if difference._mem.raw == 0:
            self._log.warning(
                "Zero fill size during position flip calculation, this could be caused by"
                "a mismatch between instrument `size_precision` and a quantity `size_precision`."
            )
            return

        cdef PositionId position_id_flip = fill.position_id
        if oms_type == OmsType.HEDGING and fill.position_id.is_virtual_c():
            # Generate new position ID for flipped virtual position
            position_id_flip = self._pos_id_generator.generate(
                strategy_id=fill.strategy_id,
                flipped=True,
            )

        # Generate order fill for flipped position
        cdef OrderFilled fill_split2 = OrderFilled(
            trader_id=fill.trader_id,
            strategy_id=fill.strategy_id,
            instrument_id=fill.instrument_id,
            client_order_id=fill.client_order_id,
            venue_order_id=fill.venue_order_id,
            account_id=fill.account_id,
            trade_id=fill.trade_id,
            position_id=position_id_flip,
            order_side=fill.order_side,
            order_type=fill.order_type,
            last_qty=difference,  # Fill difference from original as above
            last_px=fill.last_px,
            currency=fill.currency,
            commission=commission2,
            liquidity_side=fill.liquidity_side,
            event_id=UUID4(),  # New event ID
            ts_event=fill.ts_event,
            ts_init=fill.ts_init,
        )

        if oms_type == OmsType.HEDGING and fill.position_id.is_virtual_c():
            self._log.warning(f"Closing position {fill_split1}.")
            self._log.warning(f"Flipping position {fill_split2}.")

        # Open flipped position
        self._open_position(instrument, None, fill_split2, oms_type)
