# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
from asyncio import Queue
from decimal import Decimal
from typing import Any, Final

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.fsm import InvalidStateTrigger
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import TradingCommand
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import ExecutionReport
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import trailing_offset_type_to_str
from nautilus_trader.model.enums import trigger_type_to_str
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderEvent
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderTriggered
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import OrderUnpacker
from nautilus_trader.model.position import Position


class LiveExecutionEngine(ExecutionEngine):
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
    clock : LiveClock
        The clock for the engine.
    config : LiveExecEngineConfig, optional
        The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `LiveExecEngineConfig`.

    """

    _sentinel: Final[None] = None

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        config: LiveExecEngineConfig | None = None,
    ) -> None:
        if config is None:
            config = LiveExecEngineConfig()
        PyCondition.type(config, LiveExecEngineConfig, "config")
        super().__init__(
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        self._loop: asyncio.AbstractEventLoop = loop
        self._cmd_queue: asyncio.Queue = Queue(maxsize=config.qsize)
        self._evt_queue: asyncio.Queue = Queue(maxsize=config.qsize)

        # Async tasks
        self._cmd_queue_task: asyncio.Task | None = None
        self._evt_queue_task: asyncio.Task | None = None
        self._inflight_check_task: asyncio.Task | None = None
        self._kill: bool = False

        # Settings
        self._reconciliation: bool = config.reconciliation
        self.reconciliation_lookback_mins: int = config.reconciliation_lookback_mins or 0
        self.filter_unclaimed_external_orders: bool = config.filter_unclaimed_external_orders
        self.filter_position_reports: bool = config.filter_position_reports
        self.inflight_check_interval_ms: int = config.inflight_check_interval_ms
        self.inflight_check_threshold_ms: int = config.inflight_check_threshold_ms
        self._inflight_check_threshold_ns: int = millis_to_nanos(self.inflight_check_threshold_ms)

        self._log.info(f"{config.reconciliation=}", LogColor.BLUE)
        self._log.info(f"{config.reconciliation_lookback_mins=}", LogColor.BLUE)
        self._log.info(f"{config.filter_unclaimed_external_orders=}", LogColor.BLUE)
        self._log.info(f"{config.filter_position_reports=}", LogColor.BLUE)
        self._log.info(f"{config.inflight_check_interval_ms=}", LogColor.BLUE)
        self._log.info(f"{config.inflight_check_threshold_ms=}", LogColor.BLUE)

        # Register endpoints
        self._msgbus.register(endpoint="ExecEngine.reconcile_report", handler=self.reconcile_report)
        self._msgbus.register(
            endpoint="ExecEngine.reconcile_mass_status",
            handler=self.reconcile_mass_status,
        )

    @property
    def reconciliation(self) -> bool:
        """
        Return whether the reconciliation process will be run on start.

        Returns
        -------
        bool

        """
        return self._reconciliation

    def connect(self) -> None:
        """
        Connect the engine by calling connect on all registered clients.
        """
        if self._clients:
            self._log.info("Connecting all clients...")
        else:
            self._log.warning("No clients to connect.")
            return

        for client in self._clients.values():
            client.connect()

    def disconnect(self) -> None:
        """
        Disconnect the engine by calling disconnect on all registered clients.
        """
        if self._clients:
            self._log.info("Disconnecting all clients...")
        else:
            self._log.warning("No clients to disconnect.")
            return

        for client in self._clients.values():
            client.disconnect()

    def get_cmd_queue_task(self) -> asyncio.Task | None:
        """
        Return the internal command queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._cmd_queue_task

    def get_evt_queue_task(self) -> asyncio.Task | None:
        """
        Return the internal event queue task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._evt_queue_task

    def get_inflight_check_task(self) -> asyncio.Task | None:
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
        self._kill = True
        self.stop()
        if self._cmd_queue_task:
            self._log.debug(f"Canceling {self._cmd_queue_task.get_name()}...")
            self._cmd_queue_task.cancel()
            self._cmd_queue_task = None
        if self._evt_queue_task:
            self._log.debug(f"Canceling {self._evt_queue_task.get_name()}...")
            self._evt_queue_task.cancel()
            self._evt_queue_task = None

    def execute(self, command: TradingCommand) -> None:
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
        This method is not thread-safe and should only be called from the same thread the event
        loop is running on. Calling it from a different thread may lead to unexpected behavior.

        """
        PyCondition.not_none(command, "command")
        # Do not allow None through (None is a sentinel value which stops the queue)

        try:
            self._loop.call_soon_threadsafe(self._cmd_queue.put_nowait, command)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_cmd_queue.put` as queue full "
                f"at {self._cmd_queue.qsize():_} items.",
            )
            # Schedule the `put` operation to be executed once there is space in the queue
            self._loop.create_task(self._cmd_queue.put(command))

    def process(self, event: OrderEvent) -> None:
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
        This method is not thread-safe and should only be called from the same thread the event
        loop is running on. Calling it from a different thread may lead to unexpected behavior.

        """
        PyCondition.not_none(event, "event")

        try:
            self._loop.call_soon_threadsafe(self._evt_queue.put_nowait, event)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_evt_queue.put` as queue full "
                f"at {self._evt_queue.qsize():_} items.",
            )
            # Schedule the `put` operation to be executed once there is space in the queue
            self._loop.create_task(self._evt_queue.put(event))

    # -- INTERNAL -------------------------------------------------------------------------------------

    def _enqueue_sentinel(self) -> None:
        self._loop.call_soon_threadsafe(self._cmd_queue.put_nowait, self._sentinel)
        self._loop.call_soon_threadsafe(self._evt_queue.put_nowait, self._sentinel)
        self._log.debug("Sentinel messages placed on queues.")

    def _on_start(self) -> None:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running.")

        self._cmd_queue_task = self._loop.create_task(self._run_cmd_queue(), name="cmd_queue")
        self._evt_queue_task = self._loop.create_task(self._run_evt_queue(), name="evt_queue")
        self._log.debug(f"Scheduled {self._cmd_queue_task}.")
        self._log.debug(f"Scheduled {self._evt_queue_task}.")

        if not self._inflight_check_task:
            if self.inflight_check_interval_ms > 0:
                self._inflight_check_task = self._loop.create_task(
                    self._inflight_check_loop(),
                    name="inflight_check",
                )
                self._log.debug(f"Scheduled {self._inflight_check_task}.")

    def _on_stop(self) -> None:
        if self._inflight_check_task:
            self._log.info("Canceling in-flight check task...")
            self._inflight_check_task.cancel()
            self._inflight_check_task = None

        if self._kill:
            return  # Avoids queuing redundant sentinel messages

        # This will stop the queues processing as soon as they see the sentinel message
        self._enqueue_sentinel()

    async def _wait_for_inflight_check_task(self) -> None:
        if self._inflight_check_task is None:
            return
        await self._inflight_check_task

    async def _run_cmd_queue(self) -> None:
        self._log.debug(
            f"Command message queue processing starting (qsize={self.cmd_qsize()})...",
        )
        try:
            while True:
                command: TradingCommand | None = await self._cmd_queue.get()
                if command is self._sentinel:
                    break
                self._execute_command(command)
        except asyncio.CancelledError:
            self._log.warning("Command message queue canceled.")
        except RuntimeError as e:
            self._log.error(f"RuntimeError: {e}.")
        finally:
            stopped_msg = "Command message queue stopped"
            if not self._cmd_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.cmd_qsize()} message(s) on queue.")
            else:
                self._log.debug(stopped_msg + ".")

    async def _run_evt_queue(self) -> None:
        self._log.debug(
            f"Event message queue processing starting (qsize={self.evt_qsize()})...",
        )
        try:
            while True:
                event: OrderEvent | None = await self._evt_queue.get()
                if event is self._sentinel:
                    break
                self._handle_event(event)
        except asyncio.CancelledError:
            self._log.warning("Event message queue canceled.")
        except RuntimeError as e:
            self._log.error(f"RuntimeError: {e}.")
        finally:
            stopped_msg = "Event message queue stopped"
            if not self._evt_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.evt_qsize()} message(s) on queue.")
            else:
                self._log.debug(stopped_msg + ".")

    async def _inflight_check_loop(self) -> None:
        try:
            while True:
                await asyncio.sleep(self.inflight_check_interval_ms / 1000)
                await self._check_inflight_orders()
        except asyncio.CancelledError:
            self._log.debug("In-flight check loop task canceled.")

    async def _check_inflight_orders(self) -> None:
        self._log.debug("Checking in-flight orders status...")

        inflight_orders: list[Order] = self._cache.orders_inflight()
        inflight_len = len(inflight_orders)
        self._log.debug(f"Found {inflight_len} order{'' if inflight_len == 1 else 's'} in-flight.")
        for order in inflight_orders:
            ts_now = self._clock.timestamp_ns()
            ts_init_last = order.last_event.ts_event
            self._log.debug(f"Checking in-flight order: {ts_now=}, {ts_init_last=}, {order=}...")
            if ts_now > order.last_event.ts_event + self._inflight_check_threshold_ns:
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

    # -- RECONCILIATION -------------------------------------------------------------------------------

    async def reconcile_state(self, timeout_secs: float = 10.0) -> bool:
        """
        Reconcile the internal execution state with all execution clients (external
        state).

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
        PyCondition.positive(timeout_secs, "timeout_secs")

        if not self.reconciliation:
            self._log.warning("Reconciliation deactivated.")
            return True

        results: list[bool] = []

        # Request execution mass status report from clients
        reconciliation_lookback_mins: int | None = (
            self.reconciliation_lookback_mins if self.reconciliation_lookback_mins > 0 else None
        )
        mass_status_coros = [
            c.generate_mass_status(reconciliation_lookback_mins) for c in self._clients.values()
        ]
        mass_status_all = await asyncio.gather(*mass_status_coros)

        # Reconcile each mass status with the execution engine
        for mass_status in mass_status_all:
            result = self._reconcile_mass_status(mass_status)
            results.append(result)

        return all(results)

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
        self._log.debug(f"[RECV][RPT] {report}.")
        self.report_count += 1

        self._log.info(f"Reconciling {report}.", color=LogColor.BLUE)

        if isinstance(report, OrderStatusReport):
            result = self._reconcile_order_report(report, [])  # No trades to reconcile
        elif isinstance(report, FillReport):
            result = self._reconcile_fill_report_single(report)
        elif isinstance(report, PositionStatusReport):
            result = self._reconcile_position_report(report)
        else:
            self._log.error(  # pragma: no cover (design-time error)
                f"Cannot handle unrecognized report: {report}.",  # pragma: no cover (design-time error)
            )
            return False

        self._msgbus.publish(
            topic=f"reports.execution"
            f".{report.instrument_id.venue}"
            f".{report.instrument_id.symbol}",
            msg=report,
        )

        return result

    def reconcile_mass_status(self, report: ExecutionMassStatus) -> None:
        """
        Reconcile the given execution mass status report.

        Parameters
        ----------
        report : ExecutionMassStatus
            The execution mass status report to reconcile.

        """
        self._reconcile_mass_status(report)

    def _reconcile_mass_status(
        self,
        mass_status: ExecutionMassStatus,
    ) -> bool:
        self._log.debug(f"[RECV][RPT] {mass_status}.")
        self.report_count += 1

        if mass_status is None:
            self._log.error("Error reconciling mass status (was None).")
            return False

        self._log.info(
            f"Reconciling ExecutionMassStatus for {mass_status.venue}.",
            color=LogColor.BLUE,
        )

        results: list[bool] = []
        reconciled_orders: set[ClientOrderId] = set()
        reconciled_trades: set[TradeId] = set()

        # Reconcile all reported orders
        for venue_order_id, order_report in mass_status.order_reports().items():
            trades = mass_status.fill_reports().get(venue_order_id, [])

            # Check and handle duplicate client order IDs
            client_order_id = order_report.client_order_id
            if client_order_id is not None and client_order_id in reconciled_orders:
                self._log.error(f"Duplicate {client_order_id!r} detected: {order_report}")
                continue  # Determine how to handle this

            # Check for duplicate trade IDs
            for fill_report in trades:
                if fill_report.trade_id in reconciled_trades:
                    self._log.warning(
                        f"Duplicate {fill_report.trade_id!r} detected: {fill_report}.",
                    )
                reconciled_trades.add(fill_report.trade_id)

            try:
                result = self._reconcile_order_report(order_report, trades)
            except InvalidStateTrigger as e:
                self._log.error(str(e))
                result = False
            results.append(result)
            reconciled_orders.add(order_report.client_order_id)

        if not self.filter_position_reports:
            position_reports: list[PositionStatusReport]
            # Reconcile all reported positions
            for position_reports in mass_status.position_reports().values():
                for report in position_reports:
                    result = self._reconcile_position_report(report)
                    results.append(result)

        # Publish mass status
        self._msgbus.publish(
            topic=f"reports.execution.{mass_status.venue}",
            msg=mass_status,
        )

        return all(results)

    def _reconcile_order_report(  # noqa (too complex)
        self,
        report: OrderStatusReport,
        trades: list[FillReport],
    ) -> bool:
        client_order_id: ClientOrderId = report.client_order_id
        if client_order_id is None:
            client_order_id = self._cache.client_order_id(report.venue_order_id)
            if client_order_id is None:
                # Generate external client order ID
                client_order_id = self._generate_client_order_id()
            # Assign to report
            report.client_order_id = client_order_id

        self._log.info(f"Reconciling order for {client_order_id!r}...", LogColor.BLUE)

        order: Order = self._cache.order(client_order_id)
        if order is None:
            order = self._generate_external_order(report)
            if order is None:
                # External order dropped
                return True  # No further reconciliation
            # Add to cache without determining any position ID initially
            self._cache.add_order(order)

        instrument: Instrument | None = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot reconcile order {order.client_order_id}: "
                f"instrument {order.instrument_id} not found.",
            )
            return False  # Failed

        if report.order_status == OrderStatus.REJECTED:
            if order.status != OrderStatus.REJECTED:
                self._generate_order_rejected(order, report)
            return True  # Reconciled

        if report.order_status == OrderStatus.ACCEPTED:
            if order.status != OrderStatus.ACCEPTED:
                self._generate_order_accepted(order, report)
            return True  # Reconciled

        # Order must have been accepted from this point
        if order.status in (OrderStatus.INITIALIZED, OrderStatus.SUBMITTED):
            self._generate_order_accepted(order, report)

        # Update order quantity and price differences
        if self._should_update(order, report):
            self._generate_order_updated(order, report)

        if report.order_status == OrderStatus.TRIGGERED:
            if order.status != OrderStatus.TRIGGERED:
                self._generate_order_triggered(order, report)
            return True  # Reconciled

        if report.order_status == OrderStatus.CANCELED:
            if order.status != OrderStatus.CANCELED and order.is_open:
                if report.ts_triggered > 0:
                    self._generate_order_triggered(order, report)
                # Reconcile all trades
                for trade in trades:
                    self._reconcile_fill_report(order, trade, instrument)
                self._generate_order_canceled(order, report)
            return True  # Reconciled

        if report.order_status == OrderStatus.EXPIRED:
            if order.status != OrderStatus.EXPIRED and order.is_open:
                if report.ts_triggered > 0:
                    self._generate_order_triggered(order, report)
                self._generate_order_expired(order, report)
            return True  # Reconciled

        # Order has some fills from this point

        # Reconcile all trades
        for trade in trades:
            self._reconcile_fill_report(order, trade, instrument)

        if report.avg_px is None:
            self._log.warning("report.avg_px was `None` when a value was expected.")

        # Check reported filled qty against order filled qty
        if report.filled_qty != order.filled_qty:
            # This is due to missing fill report(s), there may now be some
            # information loss if multiple fills occurred to reach the reported
            # state, or if commissions differed from the default.
            fill: OrderFilled = self._generate_inferred_fill(order, report, instrument)
            self._handle_event(fill)
            assert report.filled_qty == order.filled_qty
            if report.avg_px is not None and not math.isclose(report.avg_px, order.avg_px):
                self._log.warning(
                    f"report.avg_px {report.avg_px} != order.avg_px {order.avg_px}",
                )

        return True  # Reconciled

    def _reconcile_fill_report_single(self, report: FillReport) -> bool:
        client_order_id: ClientOrderId | None = self._cache.client_order_id(
            report.venue_order_id,
        )
        if client_order_id is None:
            self._log.error(
                f"Cannot reconcile FillReport: client order ID {client_order_id} not found.",
            )
            return False  # Failed

        order: Order | None = self._cache.order(client_order_id)
        if order is None:
            self._log.error(
                "Cannot reconcile FillReport: no order for client order ID {client_order_id}",
            )
            return False  # Failed

        instrument: Instrument | None = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot reconcile order {order.client_order_id}: "
                f"instrument {order.instrument_id} not found.",
            )
            return False  # Failed

        return self._reconcile_fill_report(order, report, instrument)

    def _reconcile_fill_report(
        self,
        order: Order,
        report: FillReport,
        instrument: Instrument,
    ) -> bool:
        if report.trade_id in order.trade_ids:
            return True  # Fill already applied (assumes consistent trades)
        try:
            self._generate_order_filled(order, report, instrument)
        except InvalidStateTrigger as e:
            self._log.error(str(e))
            return False
        # Check correct ordering of fills
        if report.ts_event < order.ts_last:
            self._log.warning(
                f"OrderFilled applied out of chronological order from {report}",
            )
        return True

    def _reconcile_position_report(self, report: PositionStatusReport) -> bool:
        if report.venue_position_id is not None:
            return self._reconcile_position_report_hedging(report)
        else:
            return self._reconcile_position_report_netting(report)

    def _reconcile_position_report_hedging(self, report: PositionStatusReport) -> bool:
        position: Position | None = self._cache.position(report.venue_position_id)
        if position is None:
            self._log.error(
                f"Cannot reconcile position: position ID {report.venue_position_id} not found.",
            )
            return False  # Failed
        position_signed_decimal_qty: Decimal = position.signed_decimal_qty()
        if position_signed_decimal_qty != report.signed_decimal_qty:
            self._log.error(
                f"Cannot reconcile {report.instrument_id} {report.venue_position_id}: position "
                f"net qty {position_signed_decimal_qty} != reported net qty {report.signed_decimal_qty}. "
                f"{report}.",
            )
            return False  # Failed

        return True  # Reconciled

    def _reconcile_position_report_netting(self, report: PositionStatusReport) -> bool:
        positions_open: list[Position] = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=report.instrument_id,
        )

        position_signed_decimal_qty: Decimal = Decimal()
        for position in positions_open:
            position_signed_decimal_qty += position.signed_decimal_qty()
        if position_signed_decimal_qty != report.signed_decimal_qty:
            self._log.error(
                f"Cannot reconcile {report.instrument_id}: position "
                f"net qty {position_signed_decimal_qty} != reported net qty {report.signed_decimal_qty}. "
                f"{report}.",
            )
            return False  # Failed

        return True  # Reconciled

    def _generate_client_order_id(self) -> ClientOrderId:
        return ClientOrderId(f"O-{UUID4().value}")

    def _generate_inferred_fill(
        self,
        order: Order,
        report: OrderStatusReport,
        instrument: Instrument,
    ) -> OrderFilled:
        # Infer liquidity side
        liquidity_side: LiquiditySide = LiquiditySide.NO_LIQUIDITY_SIDE
        if order.order_type in (
            OrderType.MARKET,
            OrderType.STOP_MARKET,
            OrderType.TRAILING_STOP_MARKET,
        ):
            liquidity_side = LiquiditySide.TAKER
        elif report.post_only:
            liquidity_side = LiquiditySide.MAKER

        # Calculate last qty
        last_qty: Quantity = instrument.make_qty(report.filled_qty - order.filled_qty)

        # Calculate last px
        if order.avg_px is None:
            last_px: Price = instrument.make_price(report.avg_px)
        else:
            report_cost: float = float(report.avg_px or 0.0) * float(report.filled_qty)
            filled_cost = order.avg_px * float(order.filled_qty)
            last_px = instrument.make_price((report_cost - filled_cost) / float(last_qty))

        notional_value: Money = instrument.notional_value(last_qty, last_px)
        commission: Money = Money(notional_value * instrument.taker_fee, instrument.quote_currency)

        filled = OrderFilled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=report.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=report.venue_order_id,
            account_id=report.account_id,
            position_id=PositionId(f"{instrument.id}-EXTERNAL"),
            trade_id=TradeId(UUID4().value),
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

    def _generate_external_order(self, report: OrderStatusReport) -> Order | None:
        self._log.info(
            f"Generating order {report.client_order_id!r}",
            color=LogColor.BLUE,
        )

        # Prepare order options
        options: dict[str, Any] = {}
        if report.price is not None:
            options["price"] = str(report.price)
        if report.trigger_price is not None:
            options["trigger_price"] = str(report.trigger_price)
            options["trigger_type"] = trigger_type_to_str(report.trigger_type)
        if report.limit_offset is not None:
            options["limit_offset"] = str(report.limit_offset)
            options["trailing_offset_type"] = trailing_offset_type_to_str(
                report.trailing_offset_type,
            )
        if report.trailing_offset is not None:
            options["trailing_offset"] = str(report.trailing_offset)
            options["trailing_offset_type"] = trailing_offset_type_to_str(
                report.trailing_offset_type,
            )
        if report.display_qty is not None:
            options["display_qty"] = str(report.display_qty)

        options["expire_time_ns"] = (
            0 if report.expire_time is None else dt_to_unix_nanos(report.expire_time)
        )

        strategy_id = self.get_external_order_claim(report.instrument_id)
        if strategy_id is None:
            strategy_id = StrategyId("EXTERNAL")
            tags = "EXTERNAL"
        else:
            tags = None

        # Check if filtering
        if self.filter_unclaimed_external_orders and strategy_id.value == "EXTERNAL":
            # Experimental: will call this out with a warning log for now
            self._log.warning(
                f"Filtering report for unclaimed EXTERNAL order, {report}.",
            )
            return None  # No further reconciliation

        initialized = OrderInitialized(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            order_side=report.order_side,
            order_type=report.order_type,
            quantity=report.quantity,
            time_in_force=report.time_in_force if report.expire_time else TimeInForce.GTC,
            post_only=report.post_only,
            reduce_only=report.reduce_only,
            quote_quantity=False,
            options=options,
            emulation_trigger=TriggerType.NO_TRIGGER,
            trigger_instrument_id=None,
            contingency_type=report.contingency_type,
            order_list_id=report.order_list_id,
            linked_order_ids=None,
            parent_order_id=None,
            exec_algorithm_id=None,
            exec_algorithm_params=None,
            exec_spawn_id=None,
            tags=tags,
            event_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            reconciliation=True,
        )

        order: Order = OrderUnpacker.from_init(initialized)
        self._log.debug(f"Generated {initialized}.")

        return order

    def _generate_order_rejected(self, order: Order, report: OrderStatusReport) -> None:
        rejected = OrderRejected(
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

    def _generate_order_accepted(self, order: Order, report: OrderStatusReport) -> None:
        accepted = OrderAccepted(
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

    def _generate_order_triggered(self, order: Order, report: OrderStatusReport) -> None:
        triggered = OrderTriggered(
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

    def _generate_order_updated(self, order: Order, report: OrderStatusReport) -> None:
        updated = OrderUpdated(
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

    def _generate_order_canceled(self, order: Order, report: OrderStatusReport) -> None:
        canceled = OrderCanceled(
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

    def _generate_order_expired(self, order: Order, report: OrderStatusReport) -> None:
        expired = OrderExpired(
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

    def _generate_order_filled(
        self,
        order: Order,
        report: FillReport,
        instrument: Instrument,
    ) -> None:
        filled = OrderFilled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=report.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=report.venue_order_id,
            account_id=report.account_id,
            trade_id=report.trade_id,
            position_id=report.venue_position_id,
            order_side=order.side,
            order_type=order.order_type,
            last_qty=report.last_qty,
            last_px=report.last_px,
            currency=instrument.quote_currency,
            commission=report.commission,
            liquidity_side=report.liquidity_side,
            event_id=UUID4(),
            ts_event=report.ts_event,
            ts_init=self._clock.timestamp_ns(),
            reconciliation=True,
        )
        self._log.debug(f"Generated {filled}.")
        self._handle_event(filled)

    def _should_update(self, order: Order, report: OrderStatusReport) -> bool:
        if report.quantity != order.quantity:
            return True

        if order.order_type == OrderType.LIMIT and report.price != order.price:
            return True

        if (
            order.order_type in [OrderType.STOP_MARKET, OrderType.TRAILING_STOP_MARKET]
            and report.trigger_price != order.trigger_price
        ):
            return True

        if order.order_type in [OrderType.STOP_LIMIT, OrderType.TRAILING_STOP_LIMIT] and (
            report.trigger_price != order.trigger_price or report.price != order.price
        ):
            return True

        return False
