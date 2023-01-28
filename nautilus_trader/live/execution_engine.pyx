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

import asyncio
import math
from typing import Optional

from nautilus_trader.config import LiveExecEngineConfig

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport RPT
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_nanos
from nautilus_trader.core.datetime cimport millis_to_nanos
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.execution.messages cimport QueryOrder
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.execution.reports cimport ExecutionMassStatus
from nautilus_trader.execution.reports cimport ExecutionReport
from nautilus_trader.execution.reports cimport OrderStatusReport
from nautilus_trader.execution.reports cimport PositionStatusReport
from nautilus_trader.execution.reports cimport TradeReport
from nautilus_trader.model.enums_c cimport LiquiditySide
from nautilus_trader.model.enums_c cimport OrderStatus
from nautilus_trader.model.enums_c cimport OrderType
from nautilus_trader.model.enums_c cimport TriggerType
from nautilus_trader.model.enums_c cimport trailing_offset_type_to_str
from nautilus_trader.model.enums_c cimport trigger_type_to_str
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderInitialized
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderTriggered
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.unpacker cimport OrderUnpacker
from nautilus_trader.model.position cimport Position
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class LiveExecutionEngine(ExecutionEngine):
    """
    Provides a high-performance asynchronous live execution engine.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the engine.
    msgbus : MessageBus
        The message bus for the engine.
    cache : Cache
        The cache for the engine.
    clock : Clock
        The clock for the engine.
    logger : Logger
        The logger for the engine.
    config : LiveExecEngineConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `LiveExecEngineConfig`.
    """
    _sentinel = None

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        config: Optional[LiveExecEngineConfig] = None,
    ):
        if config is None:
            config = LiveExecEngineConfig()
        Condition.type(config, LiveExecEngineConfig, "config")
        super().__init__(
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop = loop
        self._cmd_queue = Queue(maxsize=config.qsize)
        self._evt_queue = Queue(maxsize=config.qsize)

        # Settings
        self.reconciliation = config.reconciliation
        self.reconciliation_lookback_mins = config.reconciliation_lookback_mins or 0
        self.inflight_check_interval_ms = config.inflight_check_interval_ms
        self.inflight_check_threshold_ms = config.inflight_check_threshold_ms
        self._inflight_check_threshold_ns = millis_to_nanos(self.inflight_check_threshold_ms)

        # Async tasks
        self._cmd_queue_task = None
        self._evt_queue_task = None
        self._inflight_check_task = None
        self.is_running = False

        # Register endpoints
        self._msgbus.register(endpoint="ExecEngine.reconcile_report", handler=self.reconcile_report)
        self._msgbus.register(endpoint="ExecEngine.reconcile_mass_status", handler=self.reconcile_mass_status)

    def connect(self):
        """
        Connect the engine by calling connect on all registered clients.
        """
        self._log.info("Connecting all clients...")
        for client in self._clients.values():
            client.connect()

    def disconnect(self):
        """
        Disconnect the engine by calling disconnect on all registered clients.
        """
        self._log.info("Disconnecting all clients...")
        for client in self._clients.values():
            client.disconnect()

    def get_cmd_queue_task(self) -> Optional[asyncio.Task]:
        """
        Return the internal command queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._cmd_queue_task

    def get_evt_queue_task(self) -> Optional[asyncio.Task]:
        """
        Return the internal event queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._evt_queue_task

    def get_inflight_check_task(self) -> Optional[asyncio.Task]:
        """
        Return the internal in-flight check task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._inflight_check_task

    def cmd_qsize(self) -> int:
        """
        Return the number of `Command` messages buffered on the internal queue.

        Returns
        -------
        int

        """
        return self._cmd_queue.qsize()

    def evt_qsize(self) -> int:
        """
        Return the number of `Event` messages buffered on the internal queue.

        Returns
        -------
        int

        """
        return self._evt_queue.qsize()

# -- COMMANDS -------------------------------------------------------------------------------------

    def kill(self) -> None:
        """
        Kill the engine by abruptly canceling the queue task and calling stop.
        """
        self._log.warning("Killing engine...")
        if self._cmd_queue_task:
            self._log.debug(f"Canceling {self._cmd_queue_task.get_name()}...")
            self._cmd_queue_task.cancel()
            self._cmd_queue_task.done()
        if self._evt_queue_task:
            self._log.debug(f"Canceling {self._evt_queue_task.get_name()}...")
            self._evt_queue_task.cancel()
            self._evt_queue_task.done()
        if self.is_running:
            self.is_running = False  # Avoids sentinel messages for queues
            self.stop()

    cpdef void execute(self, TradingCommand command) except *:
        """
        Execute the given command.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        command : TradingCommand
            The command to execute.

        Warnings
        --------
        This method should only be called from the same thread the event loop is
        running on.

        """
        Condition.not_none(command, "command")
        # Do not allow None through (None is a sentinel value which stops the queue)

        try:
            self._cmd_queue.put_nowait(command)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_cmd_queue.put` as queue full "
                f"at {self._cmd_queue.qsize()} items.",
            )
            self._loop.create_task(self._cmd_queue.put(command))  # Blocking until qsize reduces

    cpdef void process(self, OrderEvent event) except *:
        """
        Process the given event.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        event : OrderEvent
            The event to process.

        Warnings
        --------
        This method should only be called from the same thread the event loop is
        running on.

        """
        Condition.not_none(event, "event")

        try:
            self._evt_queue.put_nowait(event)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_evt_queue.put` as queue full "
                f"at {self._evt_queue.qsize()} items.",
            )
            self._loop.create_task(self._evt_queue.put(event))  # Blocking until qsize reduces

# -- INTERNAL -------------------------------------------------------------------------------------

    def _enqueue_sentinel(self) -> None:
        self._cmd_queue.put_nowait(self._sentinel)
        self._evt_queue.put_nowait(self._sentinel)
        self._log.debug(f"Sentinel messages placed on queues.")

    cpdef void _on_start(self) except *:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running.")

        self.is_running = True  # Queue will continue to process
        self._cmd_queue_task = self._loop.create_task(self._run_cmd_queue(), name="cmd_queue")
        self._evt_queue_task = self._loop.create_task(self._run_evt_queue(), name="evt_queue")
        self._log.debug(f"Scheduled {self._cmd_queue_task}.")
        self._log.debug(f"Scheduled {self._evt_queue_task}.")

        if self.inflight_check_interval_ms > 0:
            self._inflight_check_task = self._loop.create_task(self._inflight_check_loop())
            self._log.debug(f"Scheduled {self._inflight_check_task}.")

    cpdef void _on_stop(self) except *:
        if self.is_running:
            self.is_running = False
            self._enqueue_sentinel()

        if self._inflight_check_task:
            self._inflight_check_task.cancel()

    async def _run_cmd_queue(self):
        self._log.debug(
            f"Command message queue processing starting (qsize={self.cmd_qsize()})...",
        )
        cdef Command command
        try:
            while self.is_running:
                command = await self._cmd_queue.get()
                if command is None:  # Sentinel message (fast C-level check)
                    continue         # Returns to the top to check `self.is_running`
                self._execute_command(command)
        except asyncio.CancelledError:
            if not self._cmd_queue.empty():
                self._log.warning(
                    f"Command message queue processing canceled "
                    f"with {self.cmd_qsize()} message(s) on queue.",
                )
            else:
                self._log.debug("Command message queue processing stopped.")

    async def _run_evt_queue(self):
        self._log.debug(
            f"Event message queue processing starting (qsize={self.evt_qsize()})...",
        )
        cdef Event event
        try:
            while self.is_running:
                event = await self._evt_queue.get()
                if event is None:  # Sentinel message (fast C-level check)
                    continue       # Returns to the top to check `self.is_running`
                self._handle_event(event)
        except asyncio.CancelledError:
            if not self._evt_queue.empty():
                self._log.warning(
                    f"Event message queue processing canceled "
                    f"with {self.evt_qsize()} message(s) on queue.",
                )
            else:
                self._log.debug("Event message queue processing stopped.")

    async def _inflight_check_loop(self) -> None:
        while True:
            await asyncio.sleep(self.inflight_check_interval_ms / 1000)
            await self._check_inflight_orders()

    async def _check_inflight_orders(self) -> None:
        self._log.debug("Checking in-flight orders status...")

        cdef list inflight_orders = self._cache.orders_inflight()
        cdef int inflight_len = len(inflight_orders)
        self._log.debug(f"Found {inflight_len} order{'' if inflight_len == 1 else 's'} in-flight.")
        cdef:
            Order order
            QueryOrder query
            uint64_t now_ns
            uint64_t ts_init_last
        for order in inflight_orders:
            now_ns = self._clock.timestamp_ns()
            ts_init_last = order.last_event_c().ts_event
            self._log.debug(f"Checking in-flight order: {now_ns=}, {ts_init_last=}, {order=}...")
            if now_ns > order.last_event_c().ts_event + self._inflight_check_threshold_ns:
                self._log.debug(f"Querying {order} with exchange...")
                query = QueryOrder(
                    trader_id=order.trader_id,
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    command_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                    )
                self._execute_command(query)

    async def reconcile_state(self, double timeout_secs=10.0) -> bool:
        """
        Reconcile the execution engines state with all execution clients.

        Parameters
        ----------
        timeout_secs : double, default 10.0
            The seconds to allow for reconciliation before timing out.

        Returns
        -------
        bool
            True if states reconcile within timeout, else False.

        Raises
        ------
        ValueError
            If `timeout_secs` is not positive (> 0).

        """
        Condition.positive(timeout_secs, "timeout_secs")

        if not self.reconciliation:
            self._log.warning("Reconciliation deactivated.")
            return True

        cdef list results = []

        # Request execution mass status report from clients
        reconciliation_lookback_mins = self.reconciliation_lookback_mins if self.reconciliation_lookback_mins > 0 else None
        mass_status_coros = [
            c.generate_mass_status(reconciliation_lookback_mins) for c in self._clients.values()
        ]
        mass_status_all = await asyncio.gather(*mass_status_coros)

        # Reconcile each mass status with the execution engine
        for mass_status in mass_status_all:
            result = self._reconcile_mass_status(mass_status)
            results.append(result)

        return all(results)

    cpdef void reconcile_report(self, ExecutionReport report) except *:
        """
        Check the given execution report.

        Parameters
        ----------
        report : ExecutionReport
            The execution report to check.

        """
        Condition.not_none(report, "report")

        self._reconcile_report(report)

    cpdef void reconcile_mass_status(self, ExecutionMassStatus report) except *:
        """
        Reconcile the given execution mass status report.

        Parameters
        ----------
        report : ExecutionMassStatus
            The execution mass status report to reconcile.

        """
        Condition.not_none(report, "report")

        self._reconcile_mass_status(report)

# -- RECONCILIATION -------------------------------------------------------------------------------

    cdef bint _reconcile_report(self, ExecutionReport report) except *:
        self._log.debug(f"{RECV}{RPT} {report}.")
        self.report_count += 1

        self._log.info(f"Reconciling {report}.", color=LogColor.BLUE)

        cdef bint result
        if isinstance(report, OrderStatusReport):
            result = self._reconcile_order_report(report, [])  # No trades to reconcile
        elif isinstance(report, TradeReport):
            result = self._reconcile_trade_report_single(report)
        elif isinstance(report, PositionStatusReport):
            result = self._reconcile_position_report(report)
        else:
            self._log.error(  # pragma: no cover (design-time error)
                f"Cannot handle report: unrecognized {report}.",  # pragma: no cover (design-time error)
            )
            return False

        self._msgbus.publish_c(
            topic=f"reports.execution"
                  f".{report.instrument_id.venue}"
                  f".{report.instrument_id.symbol}",
            msg=report,
        )

        return result

    cdef bint _reconcile_mass_status(self, ExecutionMassStatus mass_status) except *:
        self._log.debug(f"{RECV}{RPT} {mass_status}.")
        self.report_count += 1

        self._log.info(
            f"Reconciling ExecutionMassStatus for {mass_status.venue}.",
            color=LogColor.BLUE,
        )

        cdef list results = []  # type: list[bool]

        cdef:
            VenueOrderId venue_order_id
            OrderStatusReport order_report
            list trades
            bint result
        # Reconcile all reported orders
        for venue_order_id, order_report in mass_status.order_reports().items():
            trades = mass_status.trade_reports().get(venue_order_id, [])
            try:
                result = self._reconcile_order_report(order_report, trades)
            except InvalidStateTrigger as e:
                self._log.error(str(e))
                result = False
            results.append(result)

        cdef list position_reports  # type: list[PositionStatusReport]
        # Reconcile all reported positions
        for position_reports in mass_status.position_reports().values():
            for report in position_reports:
                result = self._reconcile_position_report(report)
                results.append(result)

        # Publish mass status
        self._msgbus.publish_c(
            topic=f"reports.execution.{mass_status.venue}",
            msg=mass_status,
        )

        return all(results)

    cdef bint _reconcile_order_report(self, OrderStatusReport report, list trades) except *:
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
                self._generate_order_rejected(order, report)
            return True  # Reconciled

        if report.order_status == OrderStatus.ACCEPTED:
            if order.status_c() != OrderStatus.ACCEPTED:
                self._generate_order_accepted(order, report)
            return True  # Reconciled

        # Order must have been accepted from this point
        if order.status_c() == OrderStatus.INITIALIZED or order.status_c() == OrderStatus.SUBMITTED:
            self._generate_order_accepted(order, report)

        # Update order quantity and price deltas
        if self._should_update(order, report):
            self._generate_order_updated(order, report)

        if report.order_status == OrderStatus.TRIGGERED:
            if order.status_c() != OrderStatus.TRIGGERED:
                self._generate_order_triggered(order, report)
            return True  # Reconciled

        if report.order_status == OrderStatus.CANCELED:
            if order.status_c() != OrderStatus.CANCELED:
                if report.ts_triggered > 0:
                    self._generate_order_triggered(order, report)
                self._generate_order_canceled(order, report)
            return True  # Reconciled

        if report.order_status == OrderStatus.EXPIRED:
            if order.status_c() != OrderStatus.EXPIRED:
                if report.ts_triggered > 0:
                    self._generate_order_triggered(order, report)
                self._generate_order_expired(order, report)
            return True  # Reconciled

        # Order has some fills from this point
        cdef Instrument instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot reconcile order {order.client_order_id}: "
                f"instrument {order.instrument_id} not found.",
            )
            return False  # Failed

        # Reconcile all trades
        cdef TradeReport trade
        for trade in trades:
            self._reconcile_trade_report(order, trade, instrument)

        # Check reported filled qty against order filled qty
        cdef OrderFilled fill
        if report.filled_qty != order.filled_qty:
            # This is due to missing trade report(s), there may now be some
            # information loss if multiple fills occurred to reach the reported
            # state, or if commissions differed from the default.
            fill = self._generate_inferred_fill(order, report, instrument)
            self._handle_event(fill)
            assert report.filled_qty == order.filled_qty
            if not math.isclose(report.avg_px, order.avg_px):
                self._log.warning(
                    f"report.avg_px {report.avg_px} != order.avg_px {order.avg_px}",
                )

        return True  # Reconciled

    cdef bint _reconcile_trade_report_single(self, TradeReport report) except *:
        cdef ClientOrderId client_order_id = self._cache.client_order_id(report.venue_order_id)
        if client_order_id is None:
            self._log.error(
                "Cannot reconcile TradeReport: "
                f"client order ID {client_order_id} not found.",
            )
            return False  # Failed

        cdef Order order = self._cache.order(client_order_id)
        if order is None:
            self._log.error(
                "Cannot reconcile TradeReport: "
                f"no order for client order ID {client_order_id}",
            )
            return False  # Failed

        cdef Instrument instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot reconcile order {order.client_order_id}: "
                f"instrument {order.instrument_id} not found.",
            )
            return False  # Failed

        return self._reconcile_trade_report(order, report, instrument)

    cdef bint _reconcile_trade_report(self, Order order, TradeReport report, Instrument instrument) except *:
        if report.trade_id in order.trade_ids_c():
            return True  # Fill already applied (assumes consistent trades)
        try:
            self._generate_order_filled(order, report, instrument)
        except InvalidStateTrigger as e:
            self._log.error(str(e))
            result = False
        # Check correct ordering of fills
        if report.ts_event < order.ts_last:
            self._log.warning(
                f"OrderFilled applied out of chronological order from {report}",
            )
        return True

    cdef bint _reconcile_position_report(self, PositionStatusReport report) except *:
        if report.venue_position_id is not None:
            return self._reconcile_position_report_hedging(report)
        else:
            return self._reconcile_position_report_netting(report)

    cdef bint _reconcile_position_report_hedging(self, PositionStatusReport report) except *:
        cdef Position position = self._cache.position(report.venue_position_id)
        if position is None:
            self._log.error(
                f"Cannot reconcile position: "
                f"position ID {report.venue_position_id} not found.",
            )
            return False  # Failed
        if position.net_qty != report.net_qty:
            self._log.error(
                f"Cannot reconcile position: "
                f"position ID {report.venue_position_id} "
                f"net qty {position.net_qty} != reported {report.net_qty}. "
                f"{report}.",
            )
            return False  # Failed

        return True  # Reconciled

    cdef bint _reconcile_position_report_netting(self, PositionStatusReport report) except *:
        cdef list positions_open = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=report.instrument_id,
        )
        cdef double net_qty = 0.0
        for position in positions_open:
            net_qty += position.net_qty
        if net_qty != report.net_qty:
            self._log.error(
                f"Cannot reconcile position: "
                f"{report.instrument_id} "
                f"net qty {net_qty} != reported {report.net_qty}.",
            )
            return False  # Failed

        return True  # Reconciled

    cdef ClientOrderId _generate_client_order_id(self):
        return ClientOrderId(f"O-{UUID4().value}")

    cdef OrderFilled _generate_inferred_fill(
        self,
        Order order,
        OrderStatusReport report,
        Instrument instrument,
    ):
        # Infer liquidity side
        cdef LiquiditySide liquidity_side = LiquiditySide.NO_LIQUIDITY_SIDE
        if (
            order.order_type == OrderType.MARKET
            or order.order_type == OrderType.STOP_MARKET
            or order.order_type == OrderType.TRAILING_STOP_MARKET
        ):
            liquidity_side = LiquiditySide.TAKER
        elif report.post_only:
            liquidity_side = LiquiditySide.MAKER

        # Calculate last qty
        cdef Quantity last_qty = instrument.make_qty(report.filled_qty - order.filled_qty)

        # Calculate last px
        cdef:
            Price last_px
            double report_cost
            double filled_cost
        if order.avg_px is None:
            last_px = instrument.make_price(report.avg_px)
        else:
            report_cost = float(report.avg_px) * report.filled_qty.as_f64_c()
            filled_cost = order.avg_px * order.filled_qty.as_f64_c()
            last_px = instrument.make_price((report_cost - filled_cost) / last_qty.as_f64_c())

        cdef Money notional_value = instrument.notional_value(last_qty, last_px)
        cdef Money commission = Money(notional_value * instrument.taker_fee, instrument.quote_currency)

        cdef OrderFilled filled = OrderFilled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=report.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=report.venue_order_id,
            account_id=report.account_id,
            position_id=PositionId(f"{instrument.id}-EXTERNAL"),
            trade_id=TradeId(UUID4().to_str()),
            order_side=order.side,
            order_type=order.order_type,
            last_qty=last_qty,
            last_px=last_px,
            currency=instrument.quote_currency,
            commission=commission,
            liquidity_side=liquidity_side,
            event_id=UUID4(),
            ts_event=report.ts_last,
            ts_init=self._clock.timestamp_ns(),
            reconciliation=True,
        )

        self._log.warning(f"Generated inferred {filled}.")
        return filled

    cdef Order _generate_external_order(self, OrderStatusReport report):
        # Prepare order options
        cdef dict options = {}
        if report.price is not None:
            options["price"] = str(report.price)
        if report.trigger_price is not None:
            options["trigger_price"] = str(report.trigger_price)
            options["trigger_type"] = trigger_type_to_str(report.trigger_type)
        if report.limit_offset is not None:
            options["limit_offset"] = str(report.limit_offset)
            options["trailing_offset_type"] =  trailing_offset_type_to_str(report.trailing_offset_type)
        if report.trailing_offset is not None:
            options["trailing_offset"] = str(report.trailing_offset)
            options["trailing_offset_type"] = trailing_offset_type_to_str(report.trailing_offset_type)
        if report.display_qty is not None:
            options["display_qty"] = str(report.display_qty)

        options["expire_time_ns"] = 0 if report.expire_time is None else dt_to_unix_nanos(report.expire_time)

        cdef OrderInitialized initialized = OrderInitialized(
            trader_id=self.trader_id,
            strategy_id=StrategyId.external_c(),
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            order_side=report.order_side,
            order_type=report.order_type,
            quantity=report.quantity,
            time_in_force=report.time_in_force,
            post_only=report.post_only,
            reduce_only=report.reduce_only,
            options=options,
            emulation_trigger=TriggerType.NO_TRIGGER,
            contingency_type=report.contingency_type,
            order_list_id=report.order_list_id,
            linked_order_ids=None,
            parent_order_id=None,
            tags="EXTERNAL",
            event_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            reconciliation=True,
        )

        cdef Order order = OrderUnpacker.from_init_c(initialized)
        self._log.debug(f"Generated {initialized}.")

        return order

    cdef void _generate_order_rejected(self, Order order, OrderStatusReport report) except *:
        cdef OrderRejected rejected = OrderRejected(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            account_id=report.account_id,
            reason=report.cancel_reason or "UNKNOWN",
            event_id=UUID4(),
            ts_event=report.ts_last,
            ts_init=self._clock.timestamp_ns(),
            reconciliation=True,
        )
        self._log.debug(f"Generated {rejected}.")
        self._handle_event(rejected)

    cdef void _generate_order_accepted(self, Order order, OrderStatusReport report) except *:
        cdef OrderAccepted accepted = OrderAccepted(
            trader_id=self.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            account_id=report.account_id,
            event_id=UUID4(),
            ts_event=report.ts_accepted,
            ts_init=self._clock.timestamp_ns(),
            reconciliation=True,
        )
        self._log.debug(f"Generated {accepted}.")
        self._handle_event(accepted)

    cdef void _generate_order_triggered(self, Order order, OrderStatusReport report) except *:
        cdef OrderTriggered triggered = OrderTriggered(
            trader_id=self.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            account_id=report.account_id,
            event_id=UUID4(),
            ts_event=report.ts_triggered,
            ts_init=self._clock.timestamp_ns(),
            reconciliation=True,
        )
        self._log.debug(f"Generated {triggered}.")
        self._handle_event(triggered)

    cdef void _generate_order_updated(self, Order order, OrderStatusReport report) except *:
        cdef OrderUpdated updated = OrderUpdated(
            trader_id=self.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            account_id=report.account_id,
            quantity=report.quantity,
            price=report.price,
            trigger_price=report.trigger_price,
            event_id=UUID4(),
            ts_event=report.ts_accepted,
            ts_init=self._clock.timestamp_ns(),
            reconciliation=True,
        )
        self._log.debug(f"Generated {updated}.")
        self._handle_event(updated)

    cdef void _generate_order_canceled(self, Order order, OrderStatusReport report) except *:
        cdef OrderCanceled canceled = OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            account_id=report.account_id,
            event_id=UUID4(),
            ts_event=report.ts_last,
            ts_init=self._clock.timestamp_ns(),
            reconciliation=True,
        )
        self._log.debug(f"Generated {canceled}.")
        self._handle_event(canceled)

    cdef void _generate_order_expired(self, Order order, OrderStatusReport report) except *:
        cdef OrderExpired expired = OrderExpired(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            account_id=report.account_id,
            event_id=UUID4(),
            ts_event=report.ts_last,
            ts_init=self._clock.timestamp_ns(),
            reconciliation=True,
        )
        self._log.debug(f"Generated {expired}.")
        self._handle_event(expired)

    cdef void _generate_order_filled(self, Order order, TradeReport trade, Instrument instrument) except *:
        cdef OrderFilled filled = OrderFilled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=trade.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=trade.venue_order_id,
            account_id=trade.account_id,
            trade_id=trade.trade_id,
            position_id=trade.venue_position_id,
            order_side=order.side,
            order_type=order.order_type,
            last_qty=trade.last_qty,
            last_px=trade.last_px,
            currency=instrument.quote_currency,
            commission=trade.commission,
            liquidity_side=trade.liquidity_side,
            event_id=UUID4(),
            ts_event=trade.ts_event,
            ts_init=self._clock.timestamp_ns(),
            reconciliation=True,
        )
        self._handle_event(filled)

    cdef bint _should_update(self, Order order, OrderStatusReport report) except *:
        if report.quantity != order.quantity:
            return True
        elif order.order_type == OrderType.LIMIT:
            if report.price != order.price:
                return True
        elif order.order_type == OrderType.STOP_MARKET or order.order_type == OrderType.TRAILING_STOP_MARKET:
            if report.trigger_price != order.trigger_price:
                return True
        elif order.order_type == OrderType.STOP_LIMIT or order.order_type == OrderType.TRAILING_STOP_LIMIT:
            if report.trigger_price != order.trigger_price or report.price != order.price:
                return True
        return False
