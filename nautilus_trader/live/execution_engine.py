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

import asyncio
import math
import os
import uuid
from asyncio import Queue
from collections import Counter
from decimal import Decimal
from typing import Any
from typing import Final
from typing import cast

import pandas as pd

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.core.fsm import InvalidStateTrigger
from nautilus_trader.core.message import Command
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import GenerateExecutionMassStatus
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import ExecutionReport
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.enqueue import ThrottledEnqueuer
from nautilus_trader.live.reconciliation import calculate_reconciliation_price
from nautilus_trader.live.reconciliation import create_inferred_order_filled_event
from nautilus_trader.live.reconciliation import create_order_accepted_event
from nautilus_trader.live.reconciliation import create_order_canceled_event
from nautilus_trader.live.reconciliation import create_order_expired_event
from nautilus_trader.live.reconciliation import create_order_filled_event
from nautilus_trader.live.reconciliation import create_order_rejected_event
from nautilus_trader.live.reconciliation import create_order_triggered_event
from nautilus_trader.live.reconciliation import create_order_updated_event
from nautilus_trader.model.book import py_should_handle_own_book_order
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import trailing_offset_type_to_str
from nautilus_trader.model.enums import trigger_type_to_str
from nautilus_trader.model.events import OrderEvent
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import Instrument
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

        # Reconciliation
        self._recon_check_retries: Counter[ClientOrderId] = Counter()
        self._ts_last_query: dict[ClientOrderId, int] = {}
        self._order_local_activity_ns: dict[ClientOrderId, int] = {}
        self._inferred_fill_ts: dict[ClientOrderId, int] = {}
        self._startup_reconciliation_event: asyncio.Event = asyncio.Event()
        self._filtered_external_orders_count: int = 0

        self._cmd_enqueuer: ThrottledEnqueuer[Command] = ThrottledEnqueuer(
            qname="cmd_queue",
            queue=self._cmd_queue,
            loop=self._loop,
            clock=self._clock,
            logger=self._log,
        )
        self._evt_enqueuer: ThrottledEnqueuer[OrderEvent] = ThrottledEnqueuer(
            qname="evt_queue",
            queue=self._evt_queue,
            loop=self._loop,
            clock=self._clock,
            logger=self._log,
        )

        # Async tasks
        self._cmd_queue_task: asyncio.Task | None = None
        self._evt_queue_task: asyncio.Task | None = None
        self._reconciliation_task: asyncio.Task | None = None
        self._own_books_audit_task: asyncio.Task | None = None
        self._purge_closed_orders_task: asyncio.Task | None = None
        self._purge_closed_positions_task: asyncio.Task | None = None
        self._purge_account_events_task: asyncio.Task | None = None
        self._is_shutting_down: bool = False
        self._kill: bool = False

        # Configuration
        self._reconciliation: bool = config.reconciliation
        self.reconciliation_lookback_mins: int = config.reconciliation_lookback_mins or 0
        self.reconciliation_instrument_ids: list[InstrumentId] = (
            config.reconciliation_instrument_ids or []
        )
        self.filter_unclaimed_external_orders: bool = config.filter_unclaimed_external_orders
        self.filter_position_reports: bool = config.filter_position_reports
        self.filtered_client_order_ids: list[ClientOrderId] = config.filtered_client_order_ids or []
        self.generate_missing_orders: bool = config.generate_missing_orders
        self.inflight_check_interval_ms: int = config.inflight_check_interval_ms
        self.inflight_check_threshold_ms: int = config.inflight_check_threshold_ms
        self.inflight_check_max_retries: int = config.inflight_check_retries
        self.own_books_audit_interval_secs: float | None = config.own_books_audit_interval_secs
        self.open_check_interval_secs: float | None = config.open_check_interval_secs
        self.open_check_open_only: bool = config.open_check_open_only
        self.open_check_lookback_mins: int = config.open_check_lookback_mins
        self.open_check_threshold_ms: int = config.open_check_threshold_ms
        self.open_check_missing_retries: int = config.open_check_missing_retries
        self.max_single_order_queries_per_cycle: int = config.max_single_order_queries_per_cycle
        self.single_order_query_delay_ms: int = config.single_order_query_delay_ms
        self.reconciliation_startup_delay_secs: float = config.reconciliation_startup_delay_secs
        self.purge_closed_orders_interval_mins = config.purge_closed_orders_interval_mins
        self.purge_closed_orders_buffer_mins = config.purge_closed_orders_buffer_mins
        self.purge_closed_positions_interval_mins = config.purge_closed_positions_interval_mins
        self.purge_closed_positions_buffer_mins = config.purge_closed_positions_buffer_mins
        self.purge_account_events_interval_mins = config.purge_account_events_interval_mins
        self.purge_account_events_lookback_mins = config.purge_account_events_lookback_mins
        self.purge_from_database = config.purge_from_database
        self.graceful_shutdown_on_exception: bool = config.graceful_shutdown_on_exception

        self._log.info(f"{config.reconciliation=}", LogColor.BLUE)
        self._log.info(f"{config.reconciliation_lookback_mins=}", LogColor.BLUE)
        self._log.info(f"{config.reconciliation_instrument_ids=}", LogColor.BLUE)
        self._log.info(f"{config.filter_unclaimed_external_orders=}", LogColor.BLUE)
        self._log.info(f"{config.filter_position_reports=}", LogColor.BLUE)
        self._log.info(f"{config.filtered_client_order_ids=}", LogColor.BLUE)
        self._log.info(f"{config.inflight_check_interval_ms=}", LogColor.BLUE)
        self._log.info(f"{config.inflight_check_threshold_ms=}", LogColor.BLUE)
        self._log.info(f"{config.inflight_check_retries=}", LogColor.BLUE)
        self._log.info(f"{config.own_books_audit_interval_secs=}", LogColor.BLUE)
        self._log.info(f"{config.open_check_interval_secs=}", LogColor.BLUE)
        self._log.info(f"{config.open_check_open_only=}", LogColor.BLUE)
        self._log.info(f"{config.open_check_lookback_mins=}", LogColor.BLUE)
        self._log.info(f"{config.open_check_threshold_ms=}", LogColor.BLUE)
        self._log.info(f"{config.open_check_missing_retries=}", LogColor.BLUE)
        self._log.info(f"{config.max_single_order_queries_per_cycle=}", LogColor.BLUE)
        self._log.info(f"{config.single_order_query_delay_ms=}", LogColor.BLUE)
        self._log.info(f"{config.reconciliation_startup_delay_secs=}", LogColor.BLUE)
        self._log.info(f"{config.purge_closed_orders_interval_mins=}", LogColor.BLUE)
        self._log.info(f"{config.purge_closed_orders_buffer_mins=}", LogColor.BLUE)
        self._log.info(f"{config.purge_closed_positions_interval_mins=}", LogColor.BLUE)
        self._log.info(f"{config.purge_closed_positions_buffer_mins=}", LogColor.BLUE)
        self._log.info(f"{config.purge_account_events_interval_mins=}", LogColor.BLUE)
        self._log.info(f"{config.purge_account_events_lookback_mins=}", LogColor.BLUE)
        self._log.info(f"{config.purge_from_database=}", LogColor.BLUE)
        self._log.info(f"{config.graceful_shutdown_on_exception=}", LogColor.BLUE)

        self._inflight_check_threshold_ns: int = millis_to_nanos(self.inflight_check_threshold_ms)
        self._open_check_threshold_ns: int = millis_to_nanos(self.open_check_threshold_ms)

        # Register endpoints
        self._msgbus.register(
            endpoint="ExecEngine.reconcile_execution_report",
            handler=self.reconcile_execution_report,
        )
        self._msgbus.register(
            endpoint="ExecEngine.reconcile_execution_mass_status",
            handler=self.reconcile_execution_mass_status,
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
        elif self._external_clients:
            self._log.info(
                f"Configured for external clients: {self._external_clients}",
                LogColor.BLUE,
            )
        else:
            self._log.warning("No clients to connect")
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
            self._log.warning("No clients to disconnect")
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

    def get_own_books_audit_task(self) -> asyncio.Task | None:
        """
        Return the own books audit task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._own_books_audit_task

    def get_reconciliation_task(self) -> asyncio.Task | None:
        """
        Return the continuous reconciliation task for the engine.

        Returns
        -------
        asyncio.Task or ``None``

        """
        return self._reconciliation_task

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
        self._log.warning("Killing engine")
        self._kill = True
        self.stop()

        if self._cmd_queue_task:
            self._log.debug(f"Canceling task '{self._cmd_queue_task.get_name()}'")
            self._cmd_queue_task.cancel()
            self._cmd_queue_task = None

        if self._evt_queue_task:
            self._log.debug(f"Canceling task '{self._evt_queue_task.get_name()}'")
            self._evt_queue_task.cancel()
            self._evt_queue_task = None

    def execute(self, command: Command) -> None:
        """
        Execute the given command.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        command : Command
            The command to execute.

        """
        self._cmd_enqueuer.enqueue(command)

    def process(self, event: OrderEvent) -> None:
        """
        Process the given event message.

        If the internal queue is at or near capacity, it logs a warning (throttled)
        and schedules an asynchronous `put()` operation. This ensures all messages are
        eventually enqueued and processed without blocking the caller when the queue is full.

        Parameters
        ----------
        event : OrderEvent
            The event to process.

        """
        self._record_local_activity(event)
        self._evt_enqueuer.enqueue(event)

    # -- INTERNAL -------------------------------------------------------------------------------------

    def _handle_queue_exception(self, e: Exception, queue_name: str) -> None:
        self._log.exception(
            f"Unexpected exception in {queue_name} queue processing: {e!r}",
            e,
        )
        if self.graceful_shutdown_on_exception:
            if not self._is_shutting_down:
                self._log.warning(
                    "Initiating graceful shutdown due to unexpected exception",
                )
                self.shutdown_system(
                    f"Unexpected exception in {queue_name} queue processing: {e!r}",
                )
                self._is_shutting_down = True
        else:
            self._log.error(
                "System will terminate immediately to prevent operation in degraded state",
            )
            os._exit(1)  # Immediate crash

    def _enqueue_sentinel(self) -> None:
        self._loop.call_soon_threadsafe(self._cmd_queue.put_nowait, self._sentinel)
        self._loop.call_soon_threadsafe(self._evt_queue.put_nowait, self._sentinel)
        self._log.debug("Sentinel messages placed on queues")

    def _record_local_activity(self, event: OrderEvent | None) -> None:
        if event is None:
            return
        client_order_id = event.client_order_id
        if client_order_id is None:
            return
        ts_event = event.ts_event
        if ts_event == 0:
            ts_event = self._clock.timestamp_ns()
        self._order_local_activity_ns[client_order_id] = ts_event

    def _clear_recon_tracking(
        self,
        client_order_id: ClientOrderId,
        *,
        drop_last_query: bool = True,
    ) -> None:
        self._recon_check_retries.pop(client_order_id, None)
        if drop_last_query:
            self._ts_last_query.pop(client_order_id, None)

    def _handle_event_with_tracking(self, event: OrderEvent) -> None:
        self._record_local_activity(event)

        # Track inferred fill timestamps to prevent duplicate historical fills
        if isinstance(event, OrderFilled) and event.reconciliation:
            client_order_id = event.client_order_id
            if client_order_id not in self._inferred_fill_ts:
                self._inferred_fill_ts[client_order_id] = event.ts_event

        self._handle_event(event)

        if event.client_order_id is None:
            return

        order = self._cache.order(event.client_order_id)
        if order and order.is_closed:
            self._clear_recon_tracking(order.client_order_id)
            self._order_local_activity_ns.pop(order.client_order_id, None)
            self._inferred_fill_ts.pop(order.client_order_id, None)

    def _on_start(self) -> None:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running")

        # Clear reconciliation event for fresh start cycle
        self._startup_reconciliation_event.clear()
        self._is_shutting_down = False

        self._cmd_queue_task = self._loop.create_task(self._run_cmd_queue(), name="cmd_queue")
        self._evt_queue_task = self._loop.create_task(self._run_evt_queue(), name="evt_queue")
        self._log.debug(f"Scheduled task '{self._cmd_queue_task.get_name()}'")
        self._log.debug(f"Scheduled task '{self._evt_queue_task.get_name()}'")

        # Start reconciliation task if any check is configured
        if (
            self.inflight_check_interval_ms > 0 or self.open_check_interval_secs
        ) and not self._reconciliation_task:
            self._reconciliation_task = self._loop.create_task(
                self._continuous_reconciliation_loop(),
                name="continuous_reconciliation",
            )
            self._log.debug(f"Scheduled task '{self._reconciliation_task.get_name()}'")
            self._log.info("Started reconciliation task", LogColor.BLUE)

        if self.own_books_audit_interval_secs and not self._own_books_audit_task:
            self._own_books_audit_task = self._loop.create_task(
                self._own_books_audit_loop(self.own_books_audit_interval_secs),
                name="own_books_audit",
            )

        if self.purge_closed_orders_interval_mins and not self._purge_closed_orders_task:
            self._purge_closed_orders_task = self._loop.create_task(
                self._purge_closed_orders_loop(self.purge_closed_orders_interval_mins),
                name="purge_closed_orders",
            )

        if self.purge_closed_positions_interval_mins and not self._purge_closed_positions_task:
            self._purge_closed_positions_task = self._loop.create_task(
                self._purge_closed_positions_loop(self.purge_closed_positions_interval_mins),
                name="purge_closed_positions",
            )

        if self.purge_account_events_interval_mins and not self._purge_account_events_task:
            self._purge_account_events_task = self._loop.create_task(
                self._purge_account_events_loop(self.purge_account_events_interval_mins),
                name="purge_account_events",
            )

    def _on_stop(self) -> None:
        self._is_shutting_down = True

        if self._reconciliation_task:
            self._log.debug(f"Canceling task '{self._reconciliation_task.get_name()}'")
            self._reconciliation_task.cancel()
            self._reconciliation_task = None

        if self._own_books_audit_task:
            self._log.debug(f"Canceling task '{self._own_books_audit_task.get_name()}'")
            self._own_books_audit_task.cancel()
            self._own_books_audit_task = None

        if self._purge_closed_orders_task:
            self._log.debug(f"Canceling task '{self._purge_closed_orders_task.get_name()}'")
            self._purge_closed_orders_task.cancel()
            self._purge_closed_orders_task = None

        if self._filtered_external_orders_count > 0:
            self._log.info(
                f"Filtered {self._filtered_external_orders_count:,} unclaimed EXTERNAL orders during run",
                LogColor.BLUE,
            )

        if self._purge_closed_positions_task:
            self._log.debug(f"Canceling task '{self._purge_closed_positions_task.get_name()}'")
            self._purge_closed_positions_task.cancel()
            self._purge_closed_positions_task = None

        if self._purge_account_events_task:
            self._log.debug(f"Canceling task '{self._purge_account_events_task.get_name()}'")
            self._purge_account_events_task.cancel()
            self._purge_account_events_task = None

        if self._kill:
            return  # Avoids enqueuing unnecessary sentinel messages when termination already signaled

        # This will stop queue processing as soon as they 'see' the sentinel message
        self._enqueue_sentinel()

    async def _run_cmd_queue(self) -> None:
        self._log.debug(
            f"Command message queue processing starting (qsize={self.cmd_qsize()})",
        )
        try:
            while True:
                try:
                    command: Command | None = await self._cmd_queue.get()
                    if command is self._sentinel:
                        break

                    self._execute_command(command)
                except asyncio.CancelledError:
                    self._log.warning("Canceled task 'run_cmd_queue'")
                    break
                except Exception as e:
                    self._handle_queue_exception(e, "command")
        finally:
            stopped_msg = "Command message queue stopped"

            if not self._cmd_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.cmd_qsize()} message(s) on queue")
            else:
                self._log.debug(stopped_msg)

    async def _run_evt_queue(self) -> None:
        self._log.debug(
            f"Event message queue processing starting (qsize={self.evt_qsize()})",
        )
        try:
            while True:
                try:
                    event: OrderEvent | None = await self._evt_queue.get()
                    if event is self._sentinel:
                        break

                    self._handle_event_with_tracking(event)
                except asyncio.CancelledError:
                    self._log.warning("Canceled task 'run_evt_queue'")
                    break
                except Exception as e:
                    self._handle_queue_exception(e, "event")
        finally:
            stopped_msg = "Event message queue stopped"

            if not self._evt_queue.empty():
                self._log.warning(f"{stopped_msg} with {self.evt_qsize()} message(s) on queue")
            else:
                self._log.debug(stopped_msg)

    def _resolve_inflight_order(self, order: Order) -> None:
        if not order.is_inflight:
            self._log.debug(
                f"Skipping inflight resolution for {order.client_order_id!r} - current status {order.status_string()}",
            )
            self._clear_recon_tracking(order.client_order_id)
            self._order_local_activity_ns.pop(order.client_order_id, None)
            return

        ts_now = self._clock.timestamp_ns()

        if order.status == OrderStatus.SUBMITTED:
            rejected = create_order_rejected_event(
                order=order,
                ts_now=ts_now,
                reason="UNKNOWN",
            )
            self._log.debug(f"Generated {rejected}")
            self._handle_event_with_tracking(rejected)
        elif order.status in (OrderStatus.PENDING_UPDATE, OrderStatus.PENDING_CANCEL):
            canceled = create_order_canceled_event(
                order=order,
                ts_now=ts_now,
            )
            self._log.debug(f"Generated {canceled}")
            self._handle_event_with_tracking(canceled)
        else:
            raise RuntimeError(f"Invalid status for in-flight order, was '{order.status_string()}'")

        self._clear_recon_tracking(order.client_order_id)
        self._order_local_activity_ns.pop(order.client_order_id, None)

    async def _resolve_order_not_found_at_venue(self, order: Order) -> None:
        """
        Resolve an order that exists in cache but is not found at the venue.

        This handles the case where our cache shows an order as open but the venue has
        no record of it, which typically means the order was never successfully placed
        or was rejected.

        Before marking as rejected, performs a single-order query to check if the order
        exists but was missed due to API timing/processing delays.

        """
        ts_now = self._clock.timestamp_ns()

        self._log.debug(
            f"Performing single-order query for {order.client_order_id!r} before marking as REJECTED",
            LogColor.BLUE,
        )

        client_id = self._cache.client_id(order.client_order_id)
        if client_id is None:
            self._log.warning(
                f"No client_id found for {order.client_order_id!r}, skipping targeted query",
            )
            # Skip targeted query but proceed with resolution
        else:
            client = self._clients.get(client_id)

            try:
                query_ts = self._clock.timestamp_ns()
                command = GenerateOrderStatusReport(
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    command_id=UUID4(),
                    ts_init=query_ts,
                )

                self._ts_last_query[order.client_order_id] = query_ts
                report = await client.generate_order_status_report(command)
                if report is not None:
                    self._log.info(
                        f"Found {order.client_order_id!r} via targeted query: {report.order_status}",
                        LogColor.BLUE,
                    )
                    self._reconcile_order_report(report, trades=[])
                    return  # Order found and reconciled, no need to mark as rejected
            except Exception as e:
                self._log.warning(f"Error during targeted query for {order.client_order_id!r}: {e}")

        if not order.is_open:
            self._log.debug(
                f"Skipping reconciliation for {order.client_order_id!r} - already {order.status_string()}",
            )
            self._clear_recon_tracking(order.client_order_id)
            self._order_local_activity_ns.pop(order.client_order_id, None)
            return

        if order.status == OrderStatus.ACCEPTED:
            self._log.warning(
                f"Reconciling {order.client_order_id!r}: ACCEPTED order not found at venue, marking as REJECTED",
                LogColor.YELLOW,
            )
            rejected = create_order_rejected_event(
                order=order,
                ts_now=ts_now,
                reason="ORDER_NOT_FOUND_AT_VENUE",
            )
            self._handle_event_with_tracking(rejected)
            self._clear_recon_tracking(order.client_order_id)
            self._order_local_activity_ns.pop(order.client_order_id, None)
            return

        if order.status == OrderStatus.PARTIALLY_FILLED:
            self._log.warning(
                f"Reconciling {order.client_order_id!r}: PARTIALLY_FILLED "
                f"order not found at venue, marking as CANCELED (preserving {order.filled_qty} filled quantity)",
                LogColor.YELLOW,
            )
            canceled = create_order_canceled_event(
                order=order,
                ts_now=ts_now,
            )
            self._handle_event_with_tracking(canceled)
            self._clear_recon_tracking(order.client_order_id)
            self._order_local_activity_ns.pop(order.client_order_id, None)
            return

        if order.is_inflight:
            self._log.debug(
                f"Deferring resolution for {order.client_order_id!r} - still inflight state {order.status_string()}",
            )
            self._clear_recon_tracking(order.client_order_id, drop_last_query=False)
            self._ts_last_query[order.client_order_id] = ts_now
            return

        if order.is_closed:
            if order.status == OrderStatus.FILLED:
                self._log.debug(
                    f"{order.client_order_id!r} is FILLED and not found at venue (expected behavior)",
                )
            else:
                self._log.warning(
                    f"Order {order.client_order_id!r} is already closed as {order.status_string()}, "
                    "skipping missing-order resolution",
                )
            self._clear_recon_tracking(order.client_order_id)
            self._order_local_activity_ns.pop(order.client_order_id, None)
            return

        self._log.warning(
            f"Unexpected order status {order.status_string()} "
            f"for order not found at venue: {order.client_order_id!r}",
        )
        self._clear_recon_tracking(order.client_order_id)
        self._order_local_activity_ns.pop(order.client_order_id, None)

    async def _own_books_audit_loop(self, interval_secs: float) -> None:
        try:
            while True:
                await asyncio.sleep(interval_secs)
                self._cache.audit_own_order_books()
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'own_books_audit'")
        except Exception as e:
            self._log.exception("Error auditing own books", e)

    # ruff: noqa: C901
    async def _continuous_reconciliation_loop(
        self,
    ) -> None:
        try:
            # Track last execution times (in nanoseconds)
            ts_last_inflight_check = 0
            ts_last_consistency_check = 0

            # Convert intervals to nanoseconds (handle None values)
            inflight_check_interval_ns = (
                millis_to_nanos(self.inflight_check_interval_ms)
                if self.inflight_check_interval_ms > 0
                else 0
            )
            consistency_check_interval_ns = (
                secs_to_nanos(self.open_check_interval_secs) if self.open_check_interval_secs else 0
            )

            # Determine minimum sleep interval (in seconds)
            intervals_secs: list[float] = []

            if self.inflight_check_interval_ms > 0:
                intervals_secs.append(self.inflight_check_interval_ms / 1000)
            if self.open_check_interval_secs:
                intervals_secs.append(self.open_check_interval_secs)

            min_interval_secs = min(intervals_secs) if intervals_secs else 1.0

            self._log.info(
                f"Starting continuous reconciliation with intervals: "
                f"inflight={self.inflight_check_interval_ms}ms, "
                f"consistency={self.open_check_interval_secs}s",
                LogColor.BLUE,
            )

            # Only wait if reconciliation is enabled (otherwise event never set)
            if self.reconciliation:
                self._log.info(
                    "Awaiting startup reconciliation completion before starting continuous checks",
                    LogColor.BLUE,
                )
                await self._startup_reconciliation_event.wait()
                self._log.info("Startup reconciliation completed", LogColor.GREEN)

                # Apply additional startup delay AFTER reconciliation completes
                if self.reconciliation_startup_delay_secs > 0:
                    self._log.info(
                        f"Applying post-reconciliation startup delay "
                        f"({self.reconciliation_startup_delay_secs}s)",
                        LogColor.BLUE,
                    )
                    await asyncio.sleep(self.reconciliation_startup_delay_secs)
            else:
                self._log.info(
                    "Startup reconciliation disabled, proceeding with continuous checks",
                    LogColor.BLUE,
                )

            while True:
                if self._is_shutting_down:
                    self._log.debug("Reconciliation loop exiting due to stop signal")
                    break

                ts_now = self._clock.timestamp_ns()

                # Higher-frequency in-flight check (if configured)
                if (
                    inflight_check_interval_ns > 0
                    and ts_now - ts_last_inflight_check >= inflight_check_interval_ns
                ):
                    # Check stop signal before starting check
                    if self._is_shutting_down:
                        break
                    try:
                        await self._check_inflight_orders()
                        ts_last_inflight_check = ts_now
                    except Exception as e:
                        self._log.exception("Failed in check_inflight_orders", e)

                # Lower-frequency consistency check (if configured)
                if (
                    consistency_check_interval_ns > 0
                    and ts_now - ts_last_consistency_check >= consistency_check_interval_ns
                ):
                    # Check stop signal before starting check
                    if self._is_shutting_down:
                        break
                    try:
                        await self._check_orders_consistency()
                        ts_last_consistency_check = ts_now
                    except Exception as e:
                        self._log.exception("Failed in check_orders_consistency", e)

                await asyncio.sleep(min_interval_secs)
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'continuous_reconciliation'")

    async def _check_inflight_orders(self) -> None:
        if self._is_shutting_down:
            self._log.debug("Skipping in-flight orders check due to stop signal")
            return

        self._log.debug("Checking in-flight orders status")

        delayed_orders: list[Order] = []
        inflight_orders: list[Order] = self._cache.orders_inflight()

        ts_now = self._clock.timestamp_ns()

        for order in inflight_orders:
            if ts_now > order.last_event.ts_event + self._inflight_check_threshold_ns:
                delayed_orders.append(order)

        if delayed_orders:
            self._log.debug(
                f"Detected {len(delayed_orders)} delayed in-flight "
                f"order{'' if len(delayed_orders) == 1 else 's'}",
            )

        # Query and potentially resolve each inconsistent order
        for order in delayed_orders:
            if not order.is_inflight:
                self._clear_recon_tracking(order.client_order_id, drop_last_query=False)
                continue

            last_query_ts = self._ts_last_query.get(order.client_order_id)
            if last_query_ts and ts_now - last_query_ts < self._inflight_check_threshold_ns:
                self._log.debug(
                    f"Skipping re-query for {order.client_order_id!r} - awaiting prior response",
                )
                continue

            retries = self._recon_check_retries[order.client_order_id]
            if retries >= self.inflight_check_max_retries:
                backlog = self.evt_qsize()
                if backlog > 0:
                    self._log.debug(
                        f"Deferring inflight resolution for {order.client_order_id!r} - event queue backlog {backlog}",
                    )
                    continue

                self._log.warning(
                    f"Order {order.client_order_id!r} exceeded max inflight retries ({retries}), "
                    f"resolving as failed",
                    LogColor.YELLOW,
                )
                self._resolve_inflight_order(order)
            else:
                self._log.debug(f"Querying {order} with venue...")
                query_ts = self._clock.timestamp_ns()
                query = QueryOrder(
                    trader_id=order.trader_id,
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    command_id=UUID4(),
                    ts_init=query_ts,
                )
                self._execute_command(query)
                self._ts_last_query[order.client_order_id] = query_ts
                self._recon_check_retries[order.client_order_id] = retries + 1

    async def _check_orders_consistency(self) -> None:
        try:
            if self._is_shutting_down:
                self._log.debug("Skipping order consistency check due to stop signal")
                return

            self._log.debug("Checking order consistency between cached-state and venues")

            open_order_ids: set[ClientOrderId] = self._cache.client_order_ids_open()
            open_orders: list[Order] = self._cache.orders_open()

            if self.reconciliation_instrument_ids:
                open_orders = [
                    o for o in open_orders if o.instrument_id in self.reconciliation_instrument_ids
                ]
                open_order_ids = {o.client_order_id for o in open_orders}

            open_len = len(open_orders)
            self._log.debug(f"Found {open_len} order{'' if open_len == 1 else 's'} open in cache")

            if not self._clients:
                return  # No clients to query

            # For continuous reconciliation limit lookback to configured window
            order_status_start = self._clock.utc_now() - pd.Timedelta(
                minutes=self.open_check_lookback_mins,
            )

            # Always use all registered clients for consistency checks
            clients = self._clients.values()

            tasks = [
                c.generate_order_status_reports(
                    GenerateOrderStatusReports(
                        instrument_id=None,
                        start=order_status_start,
                        end=None,
                        open_only=self.open_check_open_only,
                        command_id=UUID4(),
                        ts_init=self._clock.timestamp_ns(),
                        log_receipt_level=LogLevel.DEBUG,
                    ),
                )
                for c in clients
            ]

            order_reports_all = await asyncio.gather(*tasks, return_exceptions=True)
            all_order_reports: list[OrderStatusReport] = []

            for reports_or_exception in order_reports_all:
                if isinstance(reports_or_exception, Exception):
                    self._log.error(
                        f"Failed to generate order status reports: {reports_or_exception}",
                    )
                    continue

                reports = cast(list[OrderStatusReport], reports_or_exception)
                all_order_reports.extend(reports)

            # Build set of client order IDs reported by venue
            venue_reported_ids: set[ClientOrderId] = {
                report.client_order_id
                for report in all_order_reports
                if report.client_order_id is not None
            }

            # Reconcile each report against cache state
            for report in all_order_reports:
                is_in_open_ids = report.client_order_id in open_order_ids

                # Clear any retry counts for successfully queried orders
                if report.client_order_id:
                    self._clear_recon_tracking(report.client_order_id)
                elif report.venue_order_id:
                    # Try to map venue-only ID to client order ID and clear that retry counter
                    mapped_client_id = self._cache.client_order_id(report.venue_order_id)
                    if mapped_client_id:
                        self._clear_recon_tracking(mapped_client_id)

                # Check if we should reconcile this order
                should_reconcile = False
                reconcile_reason = ""

                if report.is_open != is_in_open_ids:
                    should_reconcile = True
                    reconcile_reason = f"venue_open={report.is_open}, cache_open={is_in_open_ids}"
                elif report.client_order_id:
                    order = self._cache.order(report.client_order_id)
                    if order:
                        # Check filled_qty mismatch, treating None as zero
                        report_filled = (
                            report.filled_qty
                            if report.filled_qty is not None
                            else Quantity.zero(order.quantity.precision)
                        )
                        if order.filled_qty != report_filled:
                            should_reconcile = True
                            reconcile_reason = f"filled_qty mismatch: venue={report_filled}, cache={order.filled_qty}"

                if should_reconcile:
                    # Apply include filter before reconciling
                    if not self._consider_for_reconciliation(report.instrument_id):
                        self._log.debug(
                            f"Skipping reconciliation for {report.client_order_id!r}: "
                            f"instrument {report.instrument_id} not in include list",
                        )
                        continue

                    self._log.debug(
                        f"Reconciling {report.client_order_id!r}: {reconcile_reason}",
                        LogColor.BLUE,
                    )
                    self._reconcile_order_report(report, trades=[])

            # When open_check_open_only=True, we can't determine if orders are truly missing
            # or just filled/canceled (since venue only returns open orders)
            # Skip the missing order resolution to avoid false positives
            if self.open_check_open_only:
                missing_orders = open_order_ids - venue_reported_ids
                if missing_orders:
                    self._log.debug(
                        f"{len(missing_orders)} cached open order(s) not in venue's current response - "
                        f"likely recently filled/canceled (venue may include recent closed orders with open query):",
                    )
                    for order_id in missing_orders:
                        self._log.debug(f"- {order_id}")

                return  # Can't reliably resolve missing orders in open_only mode

            # Check for orders that are open in cache but not reported by venue
            # In full history mode, these are genuinely missing orders
            missing_at_venue: set[ClientOrderId] = open_order_ids - venue_reported_ids
            ts_now = self._clock.timestamp_ns()

            # Track targeted queries to prevent rate limit exhaustion
            targeted_queries_count = 0
            logged_limit_warning = False

            for client_order_id in missing_at_venue:
                order = self._cache.order(client_order_id)
                if order is None:
                    self._log.error(f"{client_order_id!r} missing at venue and not found in cache")
                    continue

                # Check if order is too recent to reconcile (avoid race conditions)
                ts_last = order.ts_last
                if (ts_now - ts_last) < self._open_check_threshold_ns:
                    # TODO: Debug log for initial development only
                    self._log.debug(
                        f"Skipping reconciliation for {client_order_id!r} - order too recent "
                        f"(age={(ts_now - ts_last) / 1_000_000}ms < threshold={self.open_check_threshold_ms}ms)",
                    )
                    continue

                local_activity = self._order_local_activity_ns.get(client_order_id)
                if local_activity and (ts_now - local_activity) < self._open_check_threshold_ns:
                    self._log.debug(
                        f"Skipping reconciliation for {client_order_id!r}; "
                        f"pending local activity ({(ts_now - local_activity) / 1_000_000}ms < threshold={self.open_check_threshold_ms}ms)",
                    )
                    continue

                retries = self._recon_check_retries.get(client_order_id, 0)
                if retries >= self.open_check_missing_retries:
                    if targeted_queries_count >= self.max_single_order_queries_per_cycle:
                        self._recon_check_retries[client_order_id] = retries + 1

                        if not logged_limit_warning:
                            # Count how many orders at threshold are being deferred
                            orders_at_threshold_remaining = (
                                sum(
                                    1
                                    for cid in missing_at_venue
                                    if self._recon_check_retries.get(cid, 0)
                                    >= self.open_check_missing_retries
                                )
                                - targeted_queries_count
                            )
                            self._log.warning(
                                f"Reached max single-order queries ({self.max_single_order_queries_per_cycle}) "
                                f"this cycle, deferring {orders_at_threshold_remaining} order(s) at threshold to next cycle",
                                LogColor.YELLOW,
                            )
                            logged_limit_warning = True

                        continue  # Skip query but continue processing other orders

                    self._log.warning(
                        f"Order {client_order_id!r} not found at venue after {retries} retries, performing single-order query",
                        LogColor.YELLOW,
                    )
                    self._clear_recon_tracking(client_order_id, drop_last_query=False)
                    await self._resolve_order_not_found_at_venue(order)
                    targeted_queries_count += 1

                    # Add delay between single-order queries (skip after final query)
                    if (
                        targeted_queries_count < self.max_single_order_queries_per_cycle
                        and self.single_order_query_delay_ms > 0
                    ):
                        await asyncio.sleep(self.single_order_query_delay_ms / 1000.0)
                else:
                    self._recon_check_retries[client_order_id] = retries + 1
                    self._log.debug(
                        f"Order {client_order_id!r} not found at venue, retry {retries + 1}/{self.open_check_missing_retries}",
                    )
        except Exception as e:
            self._log.exception("Error in check_order_consistency", e)

    async def _check_open_orders(self) -> None:
        # Legacy method maintained for compatibility
        await self._check_orders_consistency()

    async def _purge_closed_orders_loop(self, interval_mins: int) -> None:
        interval_secs = interval_mins * 60
        buffer_mins = self.purge_closed_orders_buffer_mins or 0
        buffer_secs = buffer_mins * 60

        try:
            while True:
                await asyncio.sleep(interval_secs)
                ts_now = self._clock.timestamp_ns()
                self._cache.purge_closed_orders(
                    ts_now=ts_now,
                    buffer_secs=buffer_secs,
                    purge_from_database=self.purge_from_database,
                )
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'purge_closed_orders'")
        except Exception as e:
            self._log.exception("Error purging closed orders", e)

    async def _purge_closed_positions_loop(self, interval_mins: int) -> None:
        interval_secs = interval_mins * 60
        buffer_mins = self.purge_closed_positions_buffer_mins or 0
        buffer_secs = buffer_mins * 60

        try:
            while True:
                await asyncio.sleep(interval_secs)
                ts_now = self._clock.timestamp_ns()
                self._cache.purge_closed_positions(
                    ts_now=ts_now,
                    buffer_secs=buffer_secs,
                    purge_from_database=self.purge_from_database,
                )
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'purge_closed_positions'")
        except Exception as e:
            self._log.exception("Error purging closed positions", e)

    async def _purge_account_events_loop(self, interval_mins: int) -> None:
        interval_secs = interval_mins * 60
        lookback_mins = self.purge_account_events_lookback_mins or 0
        lookback_secs = lookback_mins * 60

        try:
            while True:
                await asyncio.sleep(interval_secs)
                ts_now = self._clock.timestamp_ns()
                self._cache.purge_account_events(
                    ts_now=ts_now,
                    lookback_secs=lookback_secs,
                    purge_from_database=self.purge_from_database,
                )
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'purge_account_events'")
        except Exception as e:
            self._log.exception("Error purging account events", e)

    # -- RECONCILIATION -------------------------------------------------------------------------------

    def _consider_for_reconciliation(self, instrument_id: InstrumentId) -> bool:
        if self.reconciliation_instrument_ids:
            return instrument_id in self.reconciliation_instrument_ids

        return True

    def _log_reconciliation_result(self, value: ClientId | InstrumentId, result: bool) -> None:
        if result:
            self._log.info(f"Reconciliation for {value} succeeded", LogColor.GREEN)
        else:
            self._log.warning(f"Reconciliation for {value} failed")

    def _log_skipping_reconciliation_on_instrument_id(self, report: ExecutionReport) -> None:
        self._log.debug(
            f"Skipping {type(report).__name__} reconciliation for {report.instrument_id}: "
            f"not in `reconciliation_instrument_ids` include list",
            LogColor.MAGENTA,
        )

    def _log_skipping_reconciliation_on_client_order_id(self, report: ExecutionReport) -> None:
        self._log.debug(
            f"Skipping {type(report).__name__} reconciliation for {report.client_order_id!r}: "
            f"in `filtered_client_order_ids` list",
            LogColor.MAGENTA,
        )

    def generate_execution_mass_status(self, command: GenerateExecutionMassStatus) -> None:
        self._log.info(f"Received {command!r}", LogColor.BLUE)
        self._loop.create_task(self.reconcile_execution_state())

    async def reconcile_execution_state(
        self,
        timeout_secs: float = 10.0,
    ) -> bool:
        """
        Reconcile the systems internal execution state with all execution clients
        (external state).

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

        try:
            for client_id in self._external_clients:
                command = GenerateExecutionMassStatus(
                    trader_id=self.trader_id,
                    client_id=client_id,
                    command_id=UUID4(),
                    venue=None,
                    ts_init=self._clock.timestamp_ns(),
                )
                self._log.info(
                    f"Requesting execution mass status from {client_id}",
                    LogColor.BLUE,
                )
                self._msgbus.publish(
                    topic=f"commands.trading.{client_id}",
                    msg=command,
                )

            if not self._clients:
                self._log.debug("No execution clients for reconciliation")
                # Signal completion even with no clients
                return True

            results: list[bool] = []

            # Request execution mass status report from clients
            reconciliation_lookback_mins: int | None = (
                self.reconciliation_lookback_mins if self.reconciliation_lookback_mins > 0 else None
            )
            mass_status_coros = [
                c.generate_mass_status(reconciliation_lookback_mins) for c in self._clients.values()
            ]
            mass_status_all = await asyncio.gather(*mass_status_coros, return_exceptions=True)

            # Reconcile each mass status with the execution engine
            for mass_status_or_exception in mass_status_all:
                if isinstance(mass_status_or_exception, BaseException):
                    self._log.error(f"Failed to generate mass status: {mass_status_or_exception}")
                    results.append(False)
                    continue

                if mass_status_or_exception is None:
                    self._log.warning(
                        "No execution mass status available for reconciliation "
                        "(likely due to an adapter client error when generating reports)",
                    )
                    results.append(False)
                    continue

                mass_status = cast("ExecutionMassStatus", mass_status_or_exception)
                client_id = mass_status.client_id
                venue = mass_status.venue
                result = self._reconcile_execution_mass_status(mass_status)

                if not result and self.filter_position_reports:
                    self._log_reconciliation_result(client_id, result)
                    results.append(result)
                    self._log.warning(
                        "`filter_position_reports` enabled, skipping further reconciliation",
                    )
                    continue

                client = self._clients[client_id]

                # Check internal and external position reconciliation
                report_tasks: list[asyncio.Task] = []

                for position in self._cache.positions_open(venue):
                    instrument_id = position.instrument_id

                    if instrument_id in mass_status.position_reports:
                        self._log.debug(
                            f"Position {instrument_id} for {client_id} already reconciled",
                        )
                        continue  # Already reconciled

                    self._log.info(f"{position} pending reconciliation")
                    position_status_command = GeneratePositionStatusReports(
                        instrument_id=instrument_id,
                        start=None,
                        end=None,
                        command_id=UUID4(),
                        ts_init=self._clock.timestamp_ns(),
                    )
                    report_tasks.append(
                        client.generate_position_status_reports(position_status_command),
                    )

                if report_tasks:
                    # Reconcile specific internal open positions
                    self._log.info(f"Awaiting {len(report_tasks)} position reports for {client_id}")
                    position_results: list[bool] = []

                    for task_result_or_exception in await asyncio.gather(
                        *report_tasks,
                        return_exceptions=True,
                    ):
                        if isinstance(task_result_or_exception, Exception):
                            self._log.error(
                                f"Failed to generate position status reports: {task_result_or_exception}",
                            )
                            position_results.append(False)
                            continue

                        task_result = cast("list[PositionStatusReport]", task_result_or_exception)

                        for report in task_result:
                            position_result = self._reconcile_position_report(report)
                            self._log_reconciliation_result(report.instrument_id, position_result)
                            position_results.append(position_result)

                    result = result and all(position_results)

                self._log_reconciliation_result(client_id, result)
                results.append(result)

                self._msgbus.publish(
                    topic=f"reports.execution.{mass_status.venue}",
                    msg=mass_status,
                )

            return all(results)
        finally:
            # Always signal completion to prevent continuous loop signal await hang
            self._startup_reconciliation_event.set()

    def reconcile_execution_report(self, report: ExecutionReport) -> bool:
        """
        Reconcile the given execution report.

        Parameters
        ----------
        report : ExecutionReport
            The execution report to check.

        Returns
        -------
        bool
            True if reconciliation successful, else False.

        """
        self._log.debug(f"<--[RPT] {report}")
        self.report_count += 1

        if not self._consider_for_reconciliation(report.instrument_id):
            self._log_skipping_reconciliation_on_instrument_id(report)
            return True  # Filtered

        self._log.debug(f"Reconciling {report}", color=LogColor.BLUE)

        if isinstance(report, OrderStatusReport):
            result = self._reconcile_order_report(report, [])  # No trades to reconcile
        elif isinstance(report, FillReport):
            result = self._reconcile_fill_report_single(report)
        elif isinstance(report, PositionStatusReport):
            result = self._reconcile_position_report(report)
        else:
            self._log.error(  # pragma: no cover (design-time error)
                f"Cannot handle unrecognized report: {report}",  # pragma: no cover (design-time error)
            )
            return False

        self._msgbus.publish(
            topic=f"reports.execution"
            f".{report.instrument_id.venue}"
            f".{report.instrument_id.symbol}",
            msg=report,
        )

        return result

    def reconcile_execution_mass_status(self, report: ExecutionMassStatus) -> None:
        """
        Reconcile the given execution mass status report.

        Parameters
        ----------
        report : ExecutionMassStatus
            The execution mass status report to reconcile.

        """
        self._reconcile_execution_mass_status(report)

    def _reconcile_execution_mass_status(
        self,
        mass_status: ExecutionMassStatus,
    ) -> bool:
        self._log.debug(f"<--[RPT] {mass_status}")
        self.report_count += 1

        self._log.info(
            f"Reconciling ExecutionMassStatus for {mass_status.venue}",
            color=LogColor.BLUE,
        )

        results: list[bool] = []
        reconciled_orders: set[ClientOrderId] = set()
        reconciled_trades: set[TradeId] = set()

        # Reconcile all reported orders
        for venue_order_id, order_report in mass_status.order_reports.items():
            trades = mass_status.fill_reports.get(venue_order_id, [])

            if not self._consider_for_reconciliation(order_report.instrument_id):
                self._log_skipping_reconciliation_on_instrument_id(order_report)
                continue

            # Check and handle duplicate client order IDs
            client_order_id = order_report.client_order_id

            if client_order_id is not None:
                if client_order_id in self.filtered_client_order_ids:
                    self._log_skipping_reconciliation_on_client_order_id(order_report)
                    continue

                if client_order_id in reconciled_orders:
                    self._log.error(f"Duplicate {client_order_id!r} detected: {order_report}")
                    results.append(False)
                    continue  # Determine how to handle this

            # Check for duplicate trade IDs
            for fill_report in trades:
                if fill_report.trade_id in reconciled_trades:
                    self._log.warning(
                        f"Duplicate {fill_report.trade_id!r} detected: {fill_report}",
                    )

                reconciled_trades.add(fill_report.trade_id)

            try:
                result = self._reconcile_order_report(order_report, trades)
            except InvalidStateTrigger as e:
                self._log.error(str(e))
                result = False

            results.append(result)
            if order_report.client_order_id is not None:
                reconciled_orders.add(order_report.client_order_id)

        if not self.filter_position_reports:
            position_reports: list[PositionStatusReport]

            # Reconcile all reported positions
            for position_reports in mass_status.position_reports.values():
                for report in position_reports:
                    if not self._consider_for_reconciliation(report.instrument_id):
                        self._log_skipping_reconciliation_on_instrument_id(report)
                        continue

                    result = self._reconcile_position_report(report)
                    results.append(result)

        # Publish mass status
        self._msgbus.publish(
            topic=f"reports.execution.{mass_status.venue}",
            msg=mass_status,
        )

        return all(results)

    def _reconcile_order_report(
        self,
        report: OrderStatusReport,
        trades: list[FillReport],
        is_external: bool = True,
    ) -> bool:

        client_order_id: ClientOrderId = report.client_order_id

        if client_order_id is None:
            client_order_id = self._cache.client_order_id(report.venue_order_id)

            if client_order_id is None:
                # Generate external client order ID
                client_order_id = self._generate_client_order_id()

            # Assign to report
            report.client_order_id = client_order_id

        # Reset retry count
        self._clear_recon_tracking(client_order_id)

        self._log.debug(f"Reconciling order for {client_order_id!r}", LogColor.MAGENTA)
        order: Order = self._cache.order(client_order_id)

        if order is None:
            instrument = self._cache.instrument(report.instrument_id)
            if instrument is None:
                self._log.debug(
                    f"Cannot reconcile order for {client_order_id!r}: "
                    f"instrument {report.instrument_id} not found",
                )
                return True  # Filtered instrument not loaded

            order = self._generate_order(report, is_external)

            if order is None:
                # External order dropped
                return True  # No further reconciliation

            # Add to cache without determining any position ID initially
            self._cache.add_order(order)

            if self.manage_own_order_books and py_should_handle_own_book_order(order):
                self._add_own_book_order(order)

        else:
            # Order already exists, check instrument
            instrument = self._cache.instrument(order.instrument_id)
            if instrument is None:
                self._log.debug(
                    f"Cannot reconcile order for {order.client_order_id!r}: "
                    f"instrument {order.instrument_id} not found",
                )
                return True  # Filtered instrument not loaded

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

        #######################################################################
        # Order has fills from this point
        #######################################################################

        # Reconcile all trades
        for trade in trades:
            self._reconcile_fill_report(order, trade, instrument)

        if report.avg_px is None:
            self._log.warning("report.avg_px was `None` when a value was expected")

        if report.filled_qty < order.filled_qty:
            self._log.error(
                f"report.filled_qty {report.filled_qty} < order.filled_qty {order.filled_qty}, "
                "this could potentially be caused by duplicate fills or corrupted cached state",
            )
            return False  # Failed

        if report.filled_qty > order.filled_qty:
            # Check if order is already closed to avoid duplicate inferred fills
            if order.is_closed:
                self._log.warning(  # TODO: Reduce level to debug after initial development phase
                    f"{order.client_order_id!r} already {order.status_string()} but "
                    f"reported difference in filled_qty: "
                    f"report={report.filled_qty}, cached={order.filled_qty}, "
                    f"skipping inferred fill generation for closed order",
                )
                return True  # Consider it reconciled to avoid infinite loops

            # This is due to missing fill report(s), there may now be some
            # information loss if multiple fills occurred to reach the reported
            # state, or if commissions differed from the default.
            try:
                fill: OrderFilled = self._generate_inferred_fill(order, report, instrument)
                self._handle_event_with_tracking(fill)
            except ValueError as e:
                self._log.error(
                    f"Cannot generate inferred fill for {order.client_order_id}: {e}. "
                    f"Reconciliation for this order failed.",
                )
                return False  # Failed

            if (
                report.avg_px is not None
                and order.avg_px is not None
                and not math.isclose(float(report.avg_px), float(order.avg_px))
            ):
                self._log.warning(
                    f"report.avg_px {report.avg_px} != order.avg_px {order.avg_px}, "
                    "this could potentially be caused by information loss due to inferred fills",
                )

        return True  # Reconciled

    def _reconcile_fill_report_single(self, report: FillReport) -> bool:
        if not self._consider_for_reconciliation(report.instrument_id):
            self._log_skipping_reconciliation_on_instrument_id(report)
            return True  # Filtered

        client_order_id: ClientOrderId | None = self._cache.client_order_id(
            report.venue_order_id,
        )
        if client_order_id is None:
            self._log.warning(
                f"FillReport received before OrderStatusReport for {report.venue_order_id!r}, "
                "deferring reconciliation - this may require a synthetic order",
            )
            return False  # Failed

        order: Order | None = self._cache.order(client_order_id)

        if order is None:
            self._log.warning(
                f"FillReport received before order cached for {client_order_id!r}, "
                "deferring reconciliation",
            )
            return False  # Failed

        instrument: Instrument | None = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.debug(
                f"Cannot reconcile order for {order.client_order_id!r}: "
                f"instrument {order.instrument_id} not found",
            )
            return True  # Filtered instrument not loaded

        return self._reconcile_fill_report(order, report, instrument)

    def _reconcile_fill_report(
        self,
        order: Order,
        report: FillReport,
        instrument: Instrument,
    ) -> bool:
        # Check if this fill predates an inferred reconciliation fill
        # This prevents historical fills from being applied on top of inferred fills
        client_order_id = order.client_order_id
        if client_order_id in self._inferred_fill_ts:
            earliest_inferred_ts = self._inferred_fill_ts[client_order_id]
            if report.ts_event < earliest_inferred_ts:
                self._log.debug(
                    f"Skipping historical fill {report.trade_id} (ts_event={report.ts_event}) "
                    f"for {client_order_id!r} as it predates inferred reconciliation fill "
                    f"(ts={earliest_inferred_ts}); this fill is already accounted for in the inferred fill",
                )
                return True  # Skip this fill, it's already covered by inferred fill

        if report.trade_id in order.trade_ids:
            # Fill already applied; check if data is consistent.
            # An existing fill may be sourced from the cache on start,
            # or may exist in-memory when a reconciliation is triggered.
            existing_fill = self._get_existing_fill_for_trade_id(order, report.trade_id)

            if existing_fill:
                if not self._fill_reports_equal(existing_fill, report):
                    differences: list[str] = []

                    # Last quantity
                    if existing_fill.last_qty != report.last_qty:
                        differences.append(f"qty: {existing_fill.last_qty} vs {report.last_qty}")

                    # Last price
                    if existing_fill.last_px != report.last_px:
                        differences.append(f"px: {existing_fill.last_px} vs {report.last_px}")

                    # Commission
                    if existing_fill.commission is None and report.commission is not None:
                        differences.append(f"commission: None vs {report.commission}")
                    elif existing_fill.commission is not None and report.commission is None:
                        differences.append(f"commission: {existing_fill.commission} vs None")
                    elif existing_fill.commission is not None and report.commission is not None:
                        if existing_fill.commission.currency != report.commission.currency:
                            differences.append(
                                f"commission currency: {existing_fill.commission.currency} vs {report.commission.currency}",
                            )
                        elif existing_fill.commission != report.commission:
                            differences.append(
                                f"commission: {existing_fill.commission} vs {report.commission}",
                            )

                    # Liquidity side
                    if existing_fill.liquidity_side != report.liquidity_side:
                        differences.append(
                            f"liquidity: {existing_fill.liquidity_side} vs {report.liquidity_side}",
                        )

                    # Timestamp
                    if existing_fill.ts_event != report.ts_event:
                        differences.append(
                            f"ts_event: {existing_fill.ts_event} vs {report.ts_event}",
                        )

                    self._log.warning(
                        f"Fill report data differs from existing data for trade_id {report.trade_id}, "
                        f"differences: {', '.join(differences)}; retaining cached data for consistency",
                    )

            return True  # Fill already applied, continue with existing data

        # Check if fill would cause overfill
        potential_filled_qty = order.filled_qty + report.last_qty
        if potential_filled_qty > order.quantity:
            self._log.warning(
                f"Rejecting fill that would cause overfill for {order.client_order_id!r}: "
                f"order.quantity={order.quantity}, order.filled_qty={order.filled_qty}, "
                f"fill.last_qty={report.last_qty}, would result in filled_qty={potential_filled_qty}",
            )
            return False  # Reject fill to prevent overfill

        try:
            self._generate_order_filled(order, report, instrument)
        except InvalidStateTrigger as e:
            self._log.error(str(e))
            return False
        except ValueError as e:
            # Handle the negative leaves_qty error
            self._log.exception(
                f"ValueError when applying fill to {order.client_order_id!r}: {e}",
                e,
            )
            return False

        # Check correct ordering of fills
        if report.ts_event < order.ts_last:
            self._log.warning(
                f"OrderFilled applied out of chronological order from {report}",
            )
        return True

    def _get_existing_fill_for_trade_id(
        self,
        order: Order,
        trade_id: TradeId,
    ) -> OrderFilled | None:
        for event in order.events:
            if isinstance(event, OrderFilled) and event.trade_id == trade_id:
                return event

        return None

    def _fill_reports_equal(self, cached_fill: OrderFilled, report: FillReport) -> bool:
        # Commission can be missing on reports from some venues/paths; compare safely
        if cached_fill.commission is None and report.commission is None:
            commissions_equal = True
        elif cached_fill.commission is None or report.commission is None:
            commissions_equal = False
        else:
            commissions_equal = (
                cached_fill.commission.currency == report.commission.currency
                and cached_fill.commission == report.commission
            )

        return (
            cached_fill.last_qty == report.last_qty
            and cached_fill.last_px == report.last_px
            and commissions_equal
            and cached_fill.liquidity_side == report.liquidity_side
            and cached_fill.ts_event == report.ts_event
        )

    def _reconcile_position_report(self, report: PositionStatusReport) -> bool:
        if not self._consider_for_reconciliation(report.instrument_id):
            self._log_skipping_reconciliation_on_instrument_id(report)
            return True  # Filtered

        if report.venue_position_id is not None:
            return self._reconcile_position_report_hedging(report)
        else:
            return self._reconcile_position_report_netting(report)

    def _reconcile_position_report_hedging(self, report: PositionStatusReport) -> bool:
        self._log.info(
            f"Reconciling HEDGE position for {report.instrument_id}, venue_position_id={report.venue_position_id}",
            LogColor.BLUE,
        )

        position: Position | None = self._cache.position(report.venue_position_id)

        if position is None:
            self._log.error(
                f"Cannot reconcile position: {report.venue_position_id!r} not found",
            )
            return False  # Failed

        position_signed_decimal_qty: Decimal = position.signed_decimal_qty()

        if position_signed_decimal_qty != report.signed_decimal_qty:
            self._log.error(
                f"Cannot reconcile {report.instrument_id} {report.venue_position_id!r}: position "
                f"net qty {position_signed_decimal_qty} != reported net qty {report.signed_decimal_qty},"
                f"{report}",
            )
            return False  # Failed

        return True  # Reconciled

    def _reconcile_position_report_netting(
        self,
        report: PositionStatusReport,
    ) -> bool:
        self._log.info(f"Reconciling NET position for {report.instrument_id}", LogColor.BLUE)

        instrument = self._cache.instrument(report.instrument_id)
        if instrument is None:
            self._log.debug(
                f"Cannot reconcile position for {report.instrument_id}: instrument not found",
            )
            return True  # Filtered instrument not loaded

        positions_open: list[Position] = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=report.instrument_id,
        )

        position_signed_decimal_qty: Decimal = Decimal()

        for position in positions_open:
            position_signed_decimal_qty += position.signed_decimal_qty()

        self._log.info(f"{report.signed_decimal_qty=}", LogColor.BLUE)
        self._log.info(f"{position_signed_decimal_qty=}", LogColor.BLUE)

        if position_signed_decimal_qty != report.signed_decimal_qty:
            if not self.generate_missing_orders:
                self._log.warning(
                    f"Discrepancy for {report.instrument_id} position "
                    "when `generate_missing_orders` disabled, skipping further reconciliation",
                )
                return True

            diff = abs(position_signed_decimal_qty - report.signed_decimal_qty)
            diff_quantity = Quantity(diff, instrument.size_precision)
            self._log.info(f"{diff_quantity=}", LogColor.BLUE)

            if diff_quantity == 0:
                self._log.debug(
                    f"Difference quantity rounds to zero for {instrument.id}, skipping order generation",
                )
                return True

            # Calculate current position average price if available (needed for reconciliation)
            current_avg_px = None
            if positions_open:
                # Calculate weighted average price of current positions
                total_value = Decimal(0)
                total_qty = Decimal(0)

                for pos in positions_open:
                    qty = abs(pos.signed_decimal_qty())

                    if pos.avg_px_open and qty > 0:
                        total_value += Decimal(str(pos.avg_px_open)) * qty
                        total_qty += qty

                if total_qty > 0:
                    current_avg_px = total_value / total_qty

            now = self._clock.timestamp_ns()

            # Check if position crosses through zero (flips from long to short or vice versa)
            crosses_zero = (
                position_signed_decimal_qty != 0
                and report.signed_decimal_qty != 0
                and (
                    (position_signed_decimal_qty > 0 and report.signed_decimal_qty < 0)
                    or (position_signed_decimal_qty < 0 and report.signed_decimal_qty > 0)
                )
            )

            if crosses_zero:
                self._log.info(
                    f"Position crosses through zero for {report.instrument_id}: "
                    f"current={position_signed_decimal_qty}, target={report.signed_decimal_qty}. "
                    f"Splitting reconciliation into two fills: close existing position, then open new position",
                    LogColor.BLUE,
                )

                # First fill: Close the existing position (bring to zero)
                close_qty_decimal = abs(position_signed_decimal_qty)
                close_quantity = Quantity(close_qty_decimal, instrument.size_precision)
                close_side = OrderSide.BUY if position_signed_decimal_qty < 0 else OrderSide.SELL

                # Use current position average price for closing
                close_price = None
                if current_avg_px is not None:
                    close_price = instrument.make_price(current_avg_px)
                else:
                    quote = self._cache.quote_tick(report.instrument_id)
                    if quote:
                        close_price = (
                            quote.ask_price if close_side == OrderSide.BUY else quote.bid_price
                        )

                close_result = False
                if close_price:
                    close_report = OrderStatusReport(
                        instrument_id=report.instrument_id,
                        account_id=report.account_id,
                        venue_order_id=VenueOrderId(str(uuid.uuid4())),
                        order_side=close_side,
                        order_type=OrderType.LIMIT,
                        time_in_force=TimeInForce.GTC,
                        order_status=OrderStatus.FILLED,
                        price=close_price,
                        quantity=close_quantity,
                        filled_qty=close_quantity,
                        avg_px=close_price.as_decimal(),
                        report_id=UUID4(),
                        ts_accepted=now,
                        ts_last=now,
                        ts_init=now,
                    )
                    close_result = self._reconcile_order_report(
                        close_report,
                        trades=[],
                        is_external=False,
                    )

                # Second fill: Open new position in opposite direction
                open_qty_decimal = abs(report.signed_decimal_qty)
                open_quantity = Quantity(open_qty_decimal, instrument.size_precision)
                open_side = OrderSide.BUY if report.signed_decimal_qty > 0 else OrderSide.SELL

                # Use venue's reported average price for the new position
                open_price = None
                if report.avg_px_open is not None:
                    open_price = instrument.make_price(report.avg_px_open)
                else:
                    quote = self._cache.quote_tick(report.instrument_id)
                    if quote:
                        open_price = (
                            quote.ask_price if open_side == OrderSide.BUY else quote.bid_price
                        )

                open_result = False
                if open_price:
                    open_report = OrderStatusReport(
                        instrument_id=report.instrument_id,
                        account_id=report.account_id,
                        venue_order_id=VenueOrderId(str(uuid.uuid4())),
                        order_side=open_side,
                        order_type=OrderType.LIMIT,
                        time_in_force=TimeInForce.GTC,
                        order_status=OrderStatus.FILLED,
                        price=open_price,
                        quantity=open_quantity,
                        filled_qty=open_quantity,
                        avg_px=open_price.as_decimal(),
                        report_id=UUID4(),
                        ts_accepted=now,
                        ts_last=now,
                        ts_init=now,
                    )
                    open_result = self._reconcile_order_report(
                        open_report,
                        trades=[],
                        is_external=False,
                    )

                # Check both fills succeeded
                if not (close_result and open_result):
                    self._log.error(
                        f"Failed to reconcile cross-zero position for {report.instrument_id}: "
                        f"close={close_result}, open={open_result}",
                    )
                    return False

                return True  # Reconciliation complete via split fills

            order_side = (
                OrderSide.BUY
                if report.signed_decimal_qty > position_signed_decimal_qty
                else OrderSide.SELL
            )

            # Calculate reconciliation price
            reconciliation_price = calculate_reconciliation_price(
                current_position_qty=position_signed_decimal_qty,
                current_position_avg_px=current_avg_px,
                target_position_qty=report.signed_decimal_qty,
                target_position_avg_px=report.avg_px_open,
                instrument=instrument,
            )

            # If we couldn't calculate a price, use a reasonable fallback
            if reconciliation_price is None:
                # If avg_px_open is None, we cannot compute an exact reconciliation price
                # and will fall back to a market price.
                self._log.warning(
                    f"Cannot calculate exact reconciliation price for {report.instrument_id}: "
                    f"position report lacks average price information, using last quote fallback",
                )

                quote = self._cache.quote_tick(report.instrument_id)

                if quote:
                    if order_side == OrderSide.BUY:
                        reconciliation_price = quote.ask_price
                    else:  # OrderSide.SELL
                        reconciliation_price = quote.bid_price
                else:
                    # If no market data, use current average price of positions as fallback
                    if current_avg_px is not None:
                        reconciliation_price = instrument.make_price(current_avg_px)

            if reconciliation_price:
                # Generate a LIMIT order with the calculated reconciliation price
                avg_px = reconciliation_price.as_decimal()

                diff_report = OrderStatusReport(
                    instrument_id=report.instrument_id,
                    account_id=report.account_id,
                    venue_order_id=VenueOrderId(str(uuid.uuid4())),
                    order_side=order_side,
                    order_type=OrderType.LIMIT,
                    time_in_force=TimeInForce.GTC,
                    order_status=OrderStatus.FILLED,
                    price=reconciliation_price,
                    quantity=diff_quantity,
                    filled_qty=diff_quantity,
                    avg_px=avg_px,
                    report_id=UUID4(),
                    ts_accepted=now,
                    ts_last=now,
                    ts_init=now,
                )
            else:
                # No price information, fall back to generated MARKET order
                avg_px = None
                self._log.warning(
                    f"Could not determine reconciliation price for {report.instrument_id}, "
                    f"generating MARKET order for position reconciliation "
                    f"(current: {position_signed_decimal_qty}, target: {report.signed_decimal_qty})",
                )

                diff_report = OrderStatusReport(
                    instrument_id=report.instrument_id,
                    account_id=report.account_id,
                    venue_order_id=VenueOrderId(str(uuid.uuid4())),
                    order_side=order_side,
                    order_type=OrderType.MARKET,
                    time_in_force=TimeInForce.IOC,
                    order_status=OrderStatus.FILLED,
                    quantity=diff_quantity,
                    filled_qty=diff_quantity,
                    avg_px=avg_px,
                    report_id=UUID4(),
                    ts_accepted=now,
                    ts_last=now,
                    ts_init=now,
                )

            self._reconcile_order_report(diff_report, trades=[], is_external=False)

        return True  # Reconciled

    def _generate_client_order_id(self) -> ClientOrderId:
        return ClientOrderId(f"O-{UUID4().value}")

    def _generate_inferred_fill(
        self,
        order: Order,
        report: OrderStatusReport,
        instrument: Instrument,
    ) -> OrderFilled:
        filled = create_inferred_order_filled_event(
            order=order,
            ts_now=self._clock.timestamp_ns(),
            report=report,
            instrument=instrument,
        )
        self._log.info(f"Generated inferred {filled}", LogColor.BLUE)
        return filled

    def _generate_order(
        self,
        report: OrderStatusReport,
        is_external: bool = True,
    ) -> Order | None:
        self._log.debug(f"Generating order {report.client_order_id!r}", color=LogColor.MAGENTA)

        options: dict[str, Any] = {}

        if report.price is not None:
            options["price"] = str(report.price)

        if report.trigger_price is not None:
            options["trigger_price"] = str(report.trigger_price)

        if report.trigger_type is not None:
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
            if is_external:
                # Generating from external order
                strategy_id = StrategyId("EXTERNAL")
                tags = ["EXTERNAL"]
            else:
                # Generating from internal position diff alignment
                strategy_id = StrategyId("INTERNAL-DIFF")
                tags = ["INTERNAL"]
        else:
            tags = None

        if self.filter_unclaimed_external_orders and strategy_id.is_external():
            self._filtered_external_orders_count += 1

            if self._filtered_external_orders_count == 1:
                self._log.warning("Filtering unclaimed EXTERNAL orders", LogColor.BLUE)

            return None  # No further reconciliation

        initialized = OrderInitialized(
            trader_id=self.trader_id,
            strategy_id=strategy_id,
            instrument_id=report.instrument_id,
            client_order_id=report.client_order_id,
            order_side=report.order_side,
            order_type=report.order_type,
            quantity=report.quantity,
            time_in_force=report.time_in_force,
            post_only=report.post_only,
            reduce_only=report.reduce_only,
            quote_quantity=False,
            options=options,
            emulation_trigger=TriggerType.NO_TRIGGER,
            trigger_instrument_id=None,
            contingency_type=report.contingency_type,
            order_list_id=report.order_list_id,
            linked_order_ids=report.linked_order_ids,
            parent_order_id=report.parent_order_id,
            exec_algorithm_id=None,
            exec_algorithm_params=None,
            exec_spawn_id=None,
            tags=tags,
            event_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
            reconciliation=True,
        )

        order: Order = OrderUnpacker.from_init(initialized)
        self._log.debug(f"Generated {initialized}")

        return order

    def _generate_order_rejected(self, order: Order, report: OrderStatusReport) -> None:
        rejected = create_order_rejected_event(
            order=order,
            ts_now=self._clock.timestamp_ns(),
            report=report,
        )
        self._log.debug(f"Generated {rejected}")
        self._handle_event_with_tracking(rejected)

    def _generate_order_accepted(self, order: Order, report: OrderStatusReport) -> None:
        # Clear any retry counts when order transitions to ACCEPTED
        self._clear_recon_tracking(order.client_order_id)

        # Also try to clear by venue order ID mapping
        if report.venue_order_id:
            mapped_client_id = self._cache.client_order_id(report.venue_order_id)
            if mapped_client_id:
                self._clear_recon_tracking(mapped_client_id)

        accepted = create_order_accepted_event(
            trader_id=self.trader_id,
            order=order,
            ts_now=self._clock.timestamp_ns(),
            report=report,
        )
        self._log.debug(f"Generated {accepted}")
        self._handle_event_with_tracking(accepted)

    def _generate_order_triggered(self, order: Order, report: OrderStatusReport) -> None:
        triggered = create_order_triggered_event(
            trader_id=self.trader_id,
            order=order,
            ts_now=self._clock.timestamp_ns(),
            report=report,
        )
        self._log.debug(f"Generated {triggered}")
        self._handle_event_with_tracking(triggered)

    def _generate_order_updated(self, order: Order, report: OrderStatusReport) -> None:
        updated = create_order_updated_event(
            trader_id=self.trader_id,
            order=order,
            ts_now=self._clock.timestamp_ns(),
            report=report,
        )
        self._log.debug(f"Generated {updated}")
        self._handle_event_with_tracking(updated)

    def _generate_order_canceled(self, order: Order, report: OrderStatusReport) -> None:
        canceled = create_order_canceled_event(
            order=order,
            ts_now=self._clock.timestamp_ns(),
            report=report,
        )
        self._log.debug(f"Generated {canceled}")
        self._handle_event_with_tracking(canceled)

    def _generate_order_expired(self, order: Order, report: OrderStatusReport) -> None:
        expired = create_order_expired_event(
            order=order,
            ts_now=self._clock.timestamp_ns(),
            report=report,
        )
        self._log.debug(f"Generated {expired}")
        self._handle_event_with_tracking(expired)

    def _generate_order_filled(
        self,
        order: Order,
        report: FillReport,
        instrument: Instrument,
    ) -> None:
        filled = create_order_filled_event(
            order=order,
            ts_now=self._clock.timestamp_ns(),
            report=report,
            instrument=instrument,
        )
        self._log.debug(f"Generated {filled}")
        self._handle_event_with_tracking(filled)

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
