import asyncio
import json
import math
import os
from asyncio import Queue
from collections import Counter
from collections.abc import Iterable
from dataclasses import dataclass
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
from nautilus_trader.execution.client import ExecutionClient
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.execution.messages import GenerateExecutionMassStatus
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import ExecutionReport
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from flux.events import FluxBusPayload
from flux.events import TOPIC_EXECUTION_ALERT
from nautilus_trader.live.enqueue import ThrottledEnqueuer
from nautilus_trader.persistence._execution_timing import record_command_timing
from nautilus_trader.live.reconciliation import adjust_fills_for_partial_window
from nautilus_trader.live.reconciliation import calculate_reconciliation_price
from nautilus_trader.live.reconciliation import create_inferred_order_filled_event
from nautilus_trader.live.reconciliation import create_order_accepted_event
from nautilus_trader.live.reconciliation import create_order_canceled_event
from nautilus_trader.live.reconciliation import create_order_expired_event
from nautilus_trader.live.reconciliation import create_order_filled_event
from nautilus_trader.live.reconciliation import create_order_rejected_event
from nautilus_trader.live.reconciliation import create_order_triggered_event
from nautilus_trader.live.reconciliation import create_order_updated_event
from nautilus_trader.live.reconciliation import collapse_duplicate_netting_position_reports
from nautilus_trader.live.reconciliation import get_existing_fill_for_trade_id
from nautilus_trader.live.reconciliation import is_external_reconciliation_artifact_position
from nautilus_trader.live.reconciliation import is_within_single_unit_tolerance
from nautilus_trader.model.book import py_should_handle_own_book_order
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import trailing_offset_type_to_str
from nautilus_trader.model.enums import trigger_type_to_str
from nautilus_trader.model.events import OrderEvent
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import OrderUnpacker
from nautilus_trader.model.position import Position


@dataclass(frozen=True)
class StartupOrderReference:
    client_order_id: ClientOrderId
    venue_order_id: VenueOrderId | None


@dataclass(frozen=True)
class StartupStrategyCacheSnapshot:
    account_id: AccountId | None
    instrument_id: InstrumentId
    strategy_id: StrategyId
    open_position_ids: tuple[PositionId, ...]
    open_position_qty: Decimal
    open_order_refs: tuple[StartupOrderReference, ...]
    cached_order_count: int

    @property
    def has_open_positions(self) -> bool:
        return bool(self.open_position_ids)

    @property
    def open_order_count(self) -> int:
        return len(self.open_order_refs)


@dataclass(frozen=True)
class StartupInstrumentCacheSnapshot:
    account_id: AccountId | None
    instrument_id: InstrumentId
    strategy_snapshots: tuple[StartupStrategyCacheSnapshot, ...]

    @property
    def has_open_positions(self) -> bool:
        return any(snapshot.has_open_positions for snapshot in self.strategy_snapshots)

    @property
    def total_open_order_count(self) -> int:
        return sum(snapshot.open_order_count for snapshot in self.strategy_snapshots)

    @property
    def total_cached_order_count(self) -> int:
        return sum(snapshot.cached_order_count for snapshot in self.strategy_snapshots)

    @property
    def open_order_refs(self) -> tuple[StartupOrderReference, ...]:
        refs: list[StartupOrderReference] = []
        for snapshot in self.strategy_snapshots:
            refs.extend(snapshot.open_order_refs)
        return tuple(refs)

    @property
    def startup_external_position_ids(self) -> tuple[PositionId, ...]:
        position_ids: list[PositionId] = []
        for snapshot in self.strategy_snapshots:
            if snapshot.strategy_id.value == "EXTERNAL":
                position_ids.extend(snapshot.open_position_ids)
        return tuple(position_ids)

    @property
    def startup_non_external_position_qty(self) -> Decimal:
        return sum(
            (
                snapshot.open_position_qty
                for snapshot in self.strategy_snapshots
                if snapshot.strategy_id.value != "EXTERNAL"
            ),
            start=Decimal("0"),
        )


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
    _EXECUTION_ALERT_BURST_THRESHOLD: Final[int] = 3
    _EXECUTION_ALERT_BURST_WINDOW_NS: Final[int] = 60_000_000_000
    _EXECUTION_ALERT_BURST_COOLDOWN_NS: Final[int] = 60_000_000_000

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
        self._position_local_activity_ns: dict[InstrumentId, int] = {}
        self._position_recon_retries: Counter[InstrumentId] = Counter()
        self._recent_fills_cache: dict[TradeId, int] = {}  # TradeId -> timestamp_ns (TTL cache)
        self._inferred_fill_ts: dict[ClientOrderId, int] = {}
        self._fill_application_audit: dict[ClientOrderId, list[tuple[TradeId, str, int]]] = {}
        self._startup_reconciliation_snapshot: dict[
            tuple[AccountId | None, InstrumentId, StrategyId],
            StartupStrategyCacheSnapshot,
        ] = {}
        self._startup_reconciliation_event: asyncio.Event = asyncio.Event()
        self._filtered_external_orders_count: int = 0
        self._execution_alert_windows: dict[tuple[str, str, str], list[int]] = {}
        self._execution_alert_last_sent_ns: dict[tuple[str, str, str], int] = {}

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
        self.position_check_interval_secs: float | None = config.position_check_interval_secs
        self.position_check_lookback_mins: int = config.position_check_lookback_mins
        self.position_check_threshold_ms: int = config.position_check_threshold_ms
        self.position_check_retries: int = config.position_check_retries
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
        self._log.info(f"{config.position_check_interval_secs=}", LogColor.BLUE)
        self._log.info(f"{config.position_check_lookback_mins=}", LogColor.BLUE)
        self._log.info(f"{config.position_check_threshold_ms=}", LogColor.BLUE)
        self._log.info(f"{config.position_check_retries=}", LogColor.BLUE)
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
        self._position_check_threshold_ns: int = millis_to_nanos(self.position_check_threshold_ms)

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

    # -- LIFECYCLE ---------------------------------------------------------------------------------

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
            self.inflight_check_interval_ms
            or self.open_check_interval_secs
            or self.position_check_interval_secs
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

    def _run_startup_purges(self) -> None:
        ts_now = self._clock.timestamp_ns()

        if self.purge_closed_orders_interval_mins:
            try:
                self._cache.purge_closed_orders(
                    ts_now=ts_now,
                    buffer_secs=(self.purge_closed_orders_buffer_mins or 0) * 60,
                    purge_from_database=self.purge_from_database,
                )
            except Exception as e:
                self._log.exception("Error purging closed orders on start", e)

        if self.purge_closed_positions_interval_mins:
            try:
                self._cache.purge_closed_positions(
                    ts_now=ts_now,
                    buffer_secs=(self.purge_closed_positions_buffer_mins or 0) * 60,
                    purge_from_database=self.purge_from_database,
                )
            except Exception as e:
                self._log.exception("Error purging closed positions on start", e)

        if self.purge_account_events_interval_mins:
            try:
                self._cache.purge_account_events(
                    ts_now=ts_now,
                    lookback_secs=(self.purge_account_events_lookback_mins or 0) * 60,
                    purge_from_database=self.purge_from_database,
                )
            except Exception as e:
                self._log.exception("Error purging account events on start", e)

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

    def _enqueue_sentinel(self) -> None:
        # Signal queue processing to stop
        self._loop.call_soon_threadsafe(self._cmd_queue.put_nowait, self._sentinel)
        self._loop.call_soon_threadsafe(self._evt_queue.put_nowait, self._sentinel)
        self._log.debug("Sentinel messages placed on queues")

    # -- COMMANDS ----------------------------------------------------------------------------------

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

    # -- QUEUE PROCESSING --------------------------------------------------------------------------

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
                    if isinstance(command, (SubmitOrder, SubmitOrderList, CancelOrder, BatchCancelOrders)):
                        record_command_timing(
                            command,
                            field="ts_exec_recv_ns",
                            clock=self._clock,
                        )

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

    # -- CONTINUOUS MONITORING ---------------------------------------------------------------------

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
    async def _continuous_reconciliation_loop(self) -> None:
        try:
            # Convert intervals to nanoseconds (handle None values)
            inflight_check_interval_ns = (
                millis_to_nanos(self.inflight_check_interval_ms)
                if self.inflight_check_interval_ms > 0
                else 0
            )
            consistency_check_interval_ns = (
                secs_to_nanos(self.open_check_interval_secs) if self.open_check_interval_secs else 0
            )
            position_check_interval_ns = (
                secs_to_nanos(self.position_check_interval_secs)
                if self.position_check_interval_secs
                else 0
            )
            cache_prune_interval_ns = secs_to_nanos(60.0)

            # Determine minimum sleep interval (in seconds)
            intervals_secs: list[float] = []

            if self.inflight_check_interval_ms > 0:
                intervals_secs.append(self.inflight_check_interval_ms / 1000)

            if self.open_check_interval_secs:
                intervals_secs.append(self.open_check_interval_secs)

            if self.position_check_interval_secs:
                intervals_secs.append(self.position_check_interval_secs)

            min_interval_secs = min(intervals_secs) if intervals_secs else 1.0

            self._log.info(
                f"Starting continuous reconciliation with intervals: "
                f"inflight={self.inflight_check_interval_ms}ms, "
                f"consistency={self.open_check_interval_secs}s, "
                f"position={self.position_check_interval_secs}s",
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

            # Initialize timestamps to current time so first checks wait the full interval,
            # giving execution clients time to complete their connection initialization
            ts_now_init = self._clock.timestamp_ns()
            ts_last_inflight_check = ts_now_init
            ts_last_consistency_check = ts_now_init
            ts_last_position_check = ts_now_init
            ts_last_cache_prune = ts_now_init

            while True:
                if self._is_shutting_down:
                    self._log.debug("Reconciliation loop exiting due to stop signal")
                    break

                ts_now = self._clock.timestamp_ns()

                # Check in-flight orders
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

                # Check open orders consistency
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

                # Check positions consistency
                if (
                    position_check_interval_ns > 0
                    and ts_now - ts_last_position_check >= position_check_interval_ns
                ):
                    # Check stop signal before starting check
                    if self._is_shutting_down:
                        break
                    try:
                        await self._check_positions_consistency()
                        ts_last_position_check = ts_now
                    except Exception as e:
                        self._log.exception("Failed in check_positions_consistency", e)

                if ts_now - ts_last_cache_prune >= cache_prune_interval_ns:
                    try:
                        self._prune_recent_fills_cache()
                        ts_last_cache_prune = ts_now
                    except Exception as e:
                        self._log.exception("Failed in prune_recent_fills_cache", e)

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

    async def _check_positions_consistency(self) -> None:
        if self._is_shutting_down:
            self._log.debug("Skipping position consistency check due to stop signal")
            return

        self._log.debug("Checking position consistency between cached-state and venues")

        open_positions = self._cache.positions_open()

        if self.reconciliation_instrument_ids:
            open_positions = [
                p for p in open_positions if p.instrument_id in self.reconciliation_instrument_ids
            ]

        # Group positions by instrument_id (for netting)
        positions_by_instrument: dict[InstrumentId, list[Position]] = {}

        for position in open_positions:
            if position.instrument_id not in positions_by_instrument:
                positions_by_instrument[position.instrument_id] = []

            positions_by_instrument[position.instrument_id].append(position)

        self._log.debug(
            f"Found {len(positions_by_instrument)} unique instrument(s) with open positions",
        )

        if not self._clients:
            self._log.debug("No execution clients to check position consistency, early return")
            return

        venue_positions = await self._query_position_status_reports()

        await self._process_cached_position_discrepancies(
            positions_by_instrument,
            venue_positions,
        )

        await self._process_venue_reported_positions(
            positions_by_instrument,
            venue_positions,
        )

        # Prune retry counters for instruments no longer actively discrepant
        active_instruments = set(positions_by_instrument) | set(venue_positions)
        stale = [iid for iid in self._position_recon_retries if iid not in active_instruments]
        for iid in stale:
            self._position_recon_retries.pop(iid, None)

    async def _query_position_status_reports(self) -> dict[InstrumentId, PositionStatusReport]:
        clients = self._clients.values()

        tasks = [
            c.generate_position_status_reports(
                GeneratePositionStatusReports(
                    instrument_id=None,  # Get all positions
                    start=None,  # No time filter - we want all open and closed positions
                    end=None,
                    command_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                    log_receipt_level=LogLevel.DEBUG,
                ),
            )
            for c in clients
        ]

        try:
            position_reports_all = await asyncio.gather(*tasks, return_exceptions=True)
        except Exception as e:
            self._log.error(f"Failed to gather position status reports: {e}")
            return {}

        # Build mapping: instrument_id -> venue report
        venue_positions: dict[InstrumentId, PositionStatusReport] = {}
        for reports_or_exception in position_reports_all:
            if isinstance(reports_or_exception, Exception):
                self._log.error(
                    f"Failed to generate position status reports: {reports_or_exception}",
                )
                continue

            reports = self._normalize_netting_position_reports(
                cast("list[PositionStatusReport]", reports_or_exception),
                log_prefix="Steady-state position report normalization",
            )
            for report in reports:
                venue_positions[report.instrument_id] = report

        return venue_positions

    async def _process_cached_position_discrepancies(
        self,
        positions_by_instrument: dict[InstrumentId, list[Position]],
        venue_positions: dict[InstrumentId, PositionStatusReport],
    ) -> None:
        clients = self._clients.values()

        for instrument_id, cached_positions in positions_by_instrument.items():
            venue_report = venue_positions.get(instrument_id)
            venue_qty = venue_report.signed_decimal_qty if venue_report is not None else None
            _, artifact_positions, effective_qty, raw_qty = self._effective_netting_positions_for_venue_qty(
                positions_open=cached_positions,
                instrument_id=instrument_id,
                venue_qty=venue_qty,
            )

            has_discrepancy = self._check_position_discrepancy(
                cached_positions,
                venue_report,
                instrument_id,
            )

            if not has_discrepancy and not artifact_positions:
                self._position_recon_retries.pop(instrument_id, None)
                continue

            last_activity_ts = self._position_local_activity_ns.get(instrument_id)
            if last_activity_ts:
                ts_now = self._clock.timestamp_ns()
                if ts_now - last_activity_ts < self._position_check_threshold_ns:
                    self._log.debug(
                        f"Skipping position reconciliation for {instrument_id}: "
                        f"recent activity within threshold ({self.position_check_threshold_ms}ms)",
                    )
                    continue

            retries = self._position_recon_retries[instrument_id]
            if retries >= self.position_check_retries:
                continue

            if artifact_positions and venue_report is not None:
                instrument = self._cache.instrument(instrument_id)
                if instrument is not None and self._cleanup_stale_external_reconciliation_positions(
                    report=venue_report,
                    instrument=instrument,
                    artifact_positions=artifact_positions,
                    raw_qty=raw_qty,
                    effective_qty=effective_qty,
                ):
                    self._position_recon_retries.pop(instrument_id, None)
                    continue

                self._position_recon_retries[instrument_id] = retries + 1
                if retries + 1 >= self.position_check_retries:
                    self._log.error(
                        f"Failed to clean stale EXTERNAL reconciliation artifacts for "
                        f"{instrument_id} after {self.position_check_retries} attempts "
                        f"(raw_qty={raw_qty}, effective_qty={effective_qty}, venue_qty={venue_qty})",
                    )
                continue

            if not has_discrepancy:
                self._position_recon_retries.pop(instrument_id, None)
                continue

            cached_qty = sum(p.signed_decimal_qty() for p in cached_positions)
            venue_qty = venue_qty if venue_qty is not None else Decimal(0)

            self._log.warning(
                f"Position discrepancy detected for {instrument_id}: "
                f"cached_qty={cached_qty}, venue_qty={venue_qty}; querying for missing fills...",
                LogColor.YELLOW,
            )

            missing_fills = await self._query_and_find_missing_fills(instrument_id, clients)
            await self._reconcile_missing_fills(missing_fills, instrument_id)

            # Re-read positions from cache (may have changed during reconciliation)
            current_positions = self._cache.positions_open(instrument_id=instrument_id)
            still_discrepant = self._check_position_discrepancy(
                current_positions,
                venue_report,
                instrument_id,
            )
            if still_discrepant:
                self._position_recon_retries[instrument_id] = retries + 1
                if retries + 1 >= self.position_check_retries:
                    self._log.error(
                        f"Position discrepancy for {instrument_id} unresolved after "
                        f"{self.position_check_retries} attempts "
                        f"(cached_qty={cached_qty}, venue_qty={venue_qty}); "
                        f"no further reconciliation attempts will be made",
                    )
                elif not missing_fills:
                    self._log.warning(
                        f"Position discrepancy for {instrument_id} persists but no missing fills found; "
                        f"possible causes: fills outside lookback window ({self.position_check_lookback_mins}min), "
                        f"venue position error, or internal calculation error",
                        LogColor.YELLOW,
                    )
            else:
                self._position_recon_retries.pop(instrument_id, None)

    def _check_position_discrepancy(
        self,
        cached_positions: list[Position],
        venue_report: PositionStatusReport | None,
        instrument_id: InstrumentId,
    ) -> bool:
        venue_qty = venue_report.signed_decimal_qty if venue_report is not None else None
        _, _, cached_qty, _ = self._effective_netting_positions_for_venue_qty(
            positions_open=cached_positions,
            instrument_id=instrument_id,
            venue_qty=venue_qty,
        )

        # Handle case where venue has no position report
        if venue_report is None:
            # We think we have a position, but venue says flat (or no report)
            if cached_qty != 0:
                instrument = self._cache.instrument(instrument_id)
                if instrument is not None:
                    if is_within_single_unit_tolerance(
                        cached_qty,
                        Decimal(0),
                        instrument.size_precision,
                    ):
                        return False
                else:
                    self._log.debug(
                        f"Cannot apply tolerance check for {instrument_id}: instrument not in cache",
                    )

                self._log.warning(
                    f"Position discrepancy for {instrument_id}: "
                    f"cached_qty={cached_qty}, venue has no position report",
                    LogColor.YELLOW,
                )
                return True
            # Both flat - no discrepancy
            return False

        # Check if quantities match (both could be zero)
        if cached_qty == venue_qty:
            return False

        instrument = self._cache.instrument(instrument_id)
        if instrument is not None:
            if is_within_single_unit_tolerance(
                cached_qty,
                venue_qty,
                instrument.size_precision,
            ):
                return False
        else:
            self._log.debug(
                f"Cannot apply tolerance check for {instrument_id}: instrument not in cache",
            )

        return True

    async def _process_venue_reported_positions(
        self,
        positions_by_instrument: dict[InstrumentId, list[Position]],
        venue_positions: dict[InstrumentId, PositionStatusReport],
    ) -> None:
        clients = self._clients.values()

        for instrument_id, venue_report in venue_positions.items():
            if instrument_id in positions_by_instrument:
                continue  # Already checked above

            # Apply instrument filter
            if (
                self.reconciliation_instrument_ids
                and instrument_id not in self.reconciliation_instrument_ids
            ):
                continue

            # Venue has a position but we don't - this is a discrepancy
            venue_qty = venue_report.signed_decimal_qty
            if venue_qty == 0:
                self._position_recon_retries.pop(instrument_id, None)
                continue  # Both flat, no discrepancy

            # THRESHOLD CHECK
            last_activity_ts = self._position_local_activity_ns.get(instrument_id)
            if last_activity_ts:
                ts_now = self._clock.timestamp_ns()
                if ts_now - last_activity_ts < self._position_check_threshold_ns:
                    self._log.debug(
                        f"Skipping position reconciliation for {instrument_id}: "
                        f"recent activity within threshold ({self.position_check_threshold_ms}ms)",
                    )
                    continue

            retries = self._position_recon_retries[instrument_id]
            if retries >= self.position_check_retries:
                continue

            self._log.warning(
                f"Position discrepancy detected for {instrument_id}: "
                f"cached_qty=0 (flat), venue_qty={venue_qty}; querying for missing fills...",
                LogColor.YELLOW,
            )

            missing_fills = await self._query_and_find_missing_fills(instrument_id, clients)
            await self._reconcile_missing_fills(missing_fills, instrument_id)

            # Re-check using tolerance-aware comparison
            cached_after = self._cache.positions_open(instrument_id=instrument_id)
            still_discrepant = self._check_position_discrepancy(
                cached_after,
                venue_report,
                instrument_id,
            )
            if still_discrepant:
                cached_qty_now = sum(p.signed_decimal_qty() for p in cached_after)
                self._position_recon_retries[instrument_id] = retries + 1
                if retries + 1 >= self.position_check_retries:
                    self._log.error(
                        f"Position discrepancy for {instrument_id} unresolved after "
                        f"{self.position_check_retries} attempts "
                        f"(cached_qty={cached_qty_now}, venue_qty={venue_qty}); "
                        f"no further reconciliation attempts will be made",
                    )
                elif not missing_fills:
                    self._log.warning(
                        f"Position discrepancy for {instrument_id} persists but no missing fills found; "
                        f"possible causes: fills outside lookback window ({self.position_check_lookback_mins}min), "
                        f"venue position error, or internal calculation error",
                        LogColor.YELLOW,
                    )
            else:
                self._position_recon_retries.pop(instrument_id, None)

    async def _query_and_find_missing_fills(
        self,
        instrument_id: InstrumentId,
        clients: Iterable[ExecutionClient],
    ) -> list[FillReport]:
        fill_lookback_start = self._clock.utc_now() - pd.Timedelta(
            minutes=self.position_check_lookback_mins,
        )

        fill_tasks = [
            c.generate_fill_reports(
                GenerateFillReports(
                    instrument_id=instrument_id,
                    venue_order_id=None,
                    start=fill_lookback_start,
                    end=None,
                    command_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                ),
            )
            for c in clients
        ]

        fill_reports_all = await asyncio.gather(*fill_tasks, return_exceptions=True)

        venue_fills: list[FillReport] = []
        for fills_or_exception in fill_reports_all:
            if isinstance(fills_or_exception, Exception):
                self._log.error(
                    f"Failed to generate fill reports for {instrument_id}: {fills_or_exception}",
                )
                continue

            fills = cast("list[FillReport]", fills_or_exception)
            venue_fills.extend(fills)

        cached_fill_trade_ids: set[TradeId] = set()
        for order in self._cache.orders(instrument_id=instrument_id):
            for event in order.events:
                if isinstance(event, OrderFilled):
                    cached_fill_trade_ids.add(event.trade_id)

        # Find missing fills (not in cache and not in recent fills cache)
        missing_fills = [
            fill
            for fill in venue_fills
            if fill.trade_id not in cached_fill_trade_ids
            and fill.trade_id not in self._recent_fills_cache
        ]

        return missing_fills

    async def _reconcile_missing_fills(
        self,
        missing_fills: list[FillReport],
        instrument_id: InstrumentId,
    ) -> None:
        if not missing_fills:
            return

        self._log.warning(
            f"Found {len(missing_fills)} missing fill(s) for {instrument_id}",
            LogColor.YELLOW,
        )

        for fill_report in missing_fills:
            try:
                result = self._reconcile_fill_report_single(fill_report)
                if result:
                    self._position_local_activity_ns[instrument_id] = self._clock.timestamp_ns()
                else:
                    self._log.warning(
                        f"Failed to reconcile fill {fill_report.trade_id} for {instrument_id}: "
                        f"order not yet cached or other prerequisite missing. "
                        f"Fill will be retried in next position check cycle.",
                        LogColor.YELLOW,
                    )
            except Exception as e:
                self._log.error(
                    f"Exception reconciling missing fill {fill_report.trade_id} for {instrument_id}: {e}",
                )

    def _prune_recent_fills_cache(self, ttl_secs: float = 60.0) -> None:
        # Remove expired fills from cache (default TTL: 60 seconds)
        ts_now = self._clock.timestamp_ns()
        ttl_ns = secs_to_nanos(ttl_secs)
        expired_trade_ids = [
            trade_id
            for trade_id, ts_cached in self._recent_fills_cache.items()
            if ts_now - ts_cached > ttl_ns
        ]
        for trade_id in expired_trade_ids:
            self._recent_fills_cache.pop(trade_id, None)

    async def _check_orders_consistency(self) -> None:
        try:
            if self._is_shutting_down:
                self._log.debug("Skipping order consistency check due to stop signal")
                return

            self._log.debug("Checking order consistency between cached-state and venues")

            open_order_ids: set[ClientOrderId] = self._cache.client_order_ids_open()
            inflight_order_ids: set[ClientOrderId] = self._cache.client_order_ids_inflight()

            if self.reconciliation_instrument_ids:
                open_orders: list[Order] = self._cache.orders_open()
                open_orders = [
                    o for o in open_orders if o.instrument_id in self.reconciliation_instrument_ids
                ]
                open_order_ids = {o.client_order_id for o in open_orders}
                inflight_orders: list[Order] = self._cache.orders_inflight()
                inflight_orders = [
                    o
                    for o in inflight_orders
                    if o.instrument_id in self.reconciliation_instrument_ids
                ]
                inflight_order_ids = {o.client_order_id for o in inflight_orders}

            all_order_ids = open_order_ids | inflight_order_ids
            open_len = len(all_order_ids)
            self._log.debug(f"Found {open_len} order{'' if open_len == 1 else 's'} open in cache")

            if not self._clients:
                self._log.debug("No execution clients to check orders consistency, early return")
                return

            all_order_reports, venue_reported_ids = await self._query_order_status_reports()

            self._reconcile_order_reports(all_order_reports, open_order_ids)

            if self.open_check_open_only:
                missing_orders = all_order_ids - venue_reported_ids
                if missing_orders:
                    self._log.debug(
                        f"{len(missing_orders)} cached open order(s) not in venue's current response - "
                        f"likely recently filled/canceled (venue may include recent closed orders with open query):",
                    )

                    for order_id in missing_orders:
                        self._log.debug(f"- {order_id}")

                return  # Can't reliably resolve missing orders in open_only mode

            await self._handle_missing_orders_at_venue(all_order_ids, venue_reported_ids)

            self._validate_open_orders_consistency()
        except Exception as e:
            self._log.exception("Error in check_order_consistency", e)

    def _validate_open_orders_consistency(self) -> None:
        for order in self._cache.orders_open():
            computed_filled = sum(e.last_qty for e in order.events if isinstance(e, OrderFilled))
            if computed_filled != order.filled_qty:
                self._log.error(
                    f"INCONSISTENCY: {order.client_order_id} "
                    f"computed={computed_filled} vs cached={order.filled_qty}",
                )

    async def _handle_missing_orders_at_venue(
        self,
        open_order_ids: set[ClientOrderId],
        venue_reported_ids: set[ClientOrderId],
    ) -> None:
        missing_at_venue: set[ClientOrderId] = open_order_ids - venue_reported_ids
        ts_now = self._clock.timestamp_ns()

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

    async def _resolve_order_not_found_at_venue(self, order: Order) -> None:
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

        self._resolve_cached_order_missing_at_venue(
            order,
            ts_now=ts_now,
            reason="ORDER_NOT_FOUND_AT_VENUE",
        )

    async def _query_order_status_reports(
        self,
    ) -> tuple[list[OrderStatusReport], set[ClientOrderId]]:
        order_status_start = self._clock.utc_now() - pd.Timedelta(
            minutes=self.open_check_lookback_mins,
        )

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

        venue_reported_ids: set[ClientOrderId] = {
            report.client_order_id
            for report in all_order_reports
            if report.client_order_id is not None
        }

        return all_order_reports, venue_reported_ids

    def _reconcile_order_reports(
        self,
        all_order_reports: list[OrderStatusReport],
        open_order_ids: set[ClientOrderId],
    ) -> None:
        ts_now = self._clock.timestamp_ns()

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
                        reconcile_reason = (
                            f"filled_qty mismatch: venue={report_filled}, cache={order.filled_qty}"
                        )

            if should_reconcile:
                # Apply include filter before reconciling
                if not self._consider_for_reconciliation(report.instrument_id):
                    self._log.debug(
                        f"Skipping reconciliation for {report.client_order_id!r}: "
                        f"instrument {report.instrument_id} not in include list",
                    )
                    continue

                # Check for recent local activity to avoid race conditions with in-flight fills
                local_activity = self._order_local_activity_ns.get(report.client_order_id)
                if local_activity and (ts_now - local_activity) < self._open_check_threshold_ns:
                    self._log.info(
                        f"Deferring reconciliation for {report.client_order_id!r}: "
                        f"recent local activity ({(ts_now - local_activity) / 1_000_000:.0f}ms < "
                        f"threshold={self.open_check_threshold_ms}ms), "
                        f"reason was: {reconcile_reason}",
                    )
                    continue

                self._log.debug(
                    f"Reconciling {report.client_order_id!r}: {reconcile_reason}",
                    LogColor.BLUE,
                )
                self._reconcile_order_report(report, trades=[])

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

    # -- REQUEST HANDLERS --------------------------------------------------------------------------

    def generate_execution_mass_status(self, command: GenerateExecutionMassStatus) -> None:
        """
        Handle request to generate execution mass status, triggering startup
        reconciliation.
        """
        self._log.info(f"Received {command!r}", LogColor.BLUE)
        self._loop.create_task(self.reconcile_execution_state())

    def _clear_startup_reconciliation_snapshot(self) -> None:
        self._startup_reconciliation_snapshot.clear()

    def _capture_startup_reconciliation_snapshot(self) -> None:
        snapshot_data: dict[tuple[AccountId | None, InstrumentId, StrategyId], dict[str, Any]] = {}

        for order in self._cache.orders():
            key = (
                getattr(order, "account_id", None),
                order.instrument_id,
                order.strategy_id,
            )
            entry = snapshot_data.setdefault(
                key,
                {
                    "open_position_ids": [],
                    "open_position_qty": Decimal("0"),
                    "open_order_refs": [],
                    "cached_order_count": 0,
                },
            )
            entry["cached_order_count"] += 1
            if order.is_open:
                entry["open_order_refs"].append(
                    StartupOrderReference(
                        client_order_id=order.client_order_id,
                        venue_order_id=order.venue_order_id,
                    ),
                )

        for position in self._cache.positions_open():
            key = (
                getattr(position, "account_id", None),
                position.instrument_id,
                position.strategy_id,
            )
            entry = snapshot_data.setdefault(
                key,
                {
                    "open_position_ids": [],
                    "open_position_qty": Decimal("0"),
                    "open_order_refs": [],
                    "cached_order_count": 0,
                },
            )
            entry["open_position_ids"].append(position.id)
            entry["open_position_qty"] += position.signed_decimal_qty()

        self._startup_reconciliation_snapshot = {
            key: StartupStrategyCacheSnapshot(
                account_id=key[0],
                instrument_id=key[1],
                strategy_id=key[2],
                open_position_ids=tuple(entry["open_position_ids"]),
                open_position_qty=entry["open_position_qty"],
                open_order_refs=tuple(entry["open_order_refs"]),
                cached_order_count=entry["cached_order_count"],
            )
            for key, entry in snapshot_data.items()
        }

    def _startup_snapshot_strategy_entries(
        self,
        account_id: AccountId | None,
        instrument_id: InstrumentId,
    ) -> tuple[StartupStrategyCacheSnapshot, ...]:
        return tuple(
            snapshot
            for (snapshot_account_id, snapshot_instrument_id, _strategy_id), snapshot in
            self._startup_reconciliation_snapshot.items()
            if snapshot_instrument_id == instrument_id and snapshot_account_id == account_id
        )

    def _startup_snapshot_for_instrument(
        self,
        account_id: AccountId | None,
        instrument_id: InstrumentId,
    ) -> StartupInstrumentCacheSnapshot:
        return StartupInstrumentCacheSnapshot(
            account_id=account_id,
            instrument_id=instrument_id,
            strategy_snapshots=self._startup_snapshot_strategy_entries(account_id, instrument_id),
        )

    async def _generate_startup_targeted_order_status_reports(
        self,
        client: ExecutionClient,
        instrument_id: InstrumentId,
    ) -> list[OrderStatusReport]:
        snapshot = self._startup_snapshot_for_instrument(client.account_id, instrument_id)
        if not snapshot.open_order_refs:
            return []

        seen_refs: set[tuple[ClientOrderId, VenueOrderId | None]] = set()
        commands: list[GenerateOrderStatusReport] = []
        for ref in snapshot.open_order_refs:
            ref_key = (ref.client_order_id, ref.venue_order_id)
            if ref_key in seen_refs:
                continue
            seen_refs.add(ref_key)
            commands.append(
                GenerateOrderStatusReport(
                    instrument_id=instrument_id,
                    client_order_id=ref.client_order_id,
                    venue_order_id=ref.venue_order_id,
                    command_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                ),
            )

        results = await asyncio.gather(
            *(client.generate_order_status_report(command) for command in commands),
            return_exceptions=True,
        )

        reports: list[OrderStatusReport] = []
        for command, result in zip(commands, results, strict=True):
            if isinstance(result, BaseException):
                self._log.warning(
                    f"Startup targeted order-status query failed for {command.client_order_id!r}: "
                    f"{result}",
                )
                continue
            if result is not None:
                reports.append(result)

        return reports

    @staticmethod
    def _merge_startup_order_status_reports(
        bulk_reports: list[OrderStatusReport],
        targeted_reports: list[OrderStatusReport],
    ) -> list[OrderStatusReport]:
        merged: list[OrderStatusReport] = []
        seen_keys: set[VenueOrderId | ClientOrderId] = set()
        for report in [*bulk_reports, *targeted_reports]:
            dedupe_key = report.venue_order_id or report.client_order_id
            if dedupe_key is not None and dedupe_key in seen_keys:
                continue
            if dedupe_key is not None:
                seen_keys.add(dedupe_key)
            merged.append(report)
        return merged

    async def _generate_startup_mass_status(
        self,
        client: ExecutionClient,
        reconciliation_lookback_mins: int | None,
    ) -> ExecutionMassStatus | None:
        from nautilus_trader.live.execution_client import LiveExecutionClient

        scoped_instrument_ids = self._client_scoped_reconciliation_instrument_ids(client)
        client_generate_mass_status = getattr(type(client), "generate_mass_status", None)
        uses_default_live_mass_status = (
            client_generate_mass_status is LiveExecutionClient.generate_mass_status
        )
        if not self.reconciliation_instrument_ids:
            return await client.generate_mass_status(reconciliation_lookback_mins)

        mass_status = ExecutionMassStatus(
            client_id=client.id,
            account_id=client.account_id,
            venue=client.venue,
            report_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        if not scoped_instrument_ids:
            return mass_status

        if not uses_default_live_mass_status:
            if scoped_instrument_ids:
                self._log.info(
                    f"Using adapter-defined startup ExecutionMassStatus for {client.id}; "
                    "generate_mass_status override preserved",
                    LogColor.BLUE,
                )
            return await client.generate_mass_status(reconciliation_lookback_mins)

        self._log.info(
            f"Generating scoped startup ExecutionMassStatus for {client.id} on "
            f"{len(scoped_instrument_ids)} instrument(s)",
            LogColor.BLUE,
        )

        since: pd.Timestamp | None = None
        if reconciliation_lookback_mins is not None:
            since = self._clock.utc_now() - pd.Timedelta(minutes=reconciliation_lookback_mins)

        try:
            for instrument_id in scoped_instrument_ids:
                use_open_orders_only = self._startup_order_status_should_use_open_only(
                    client=client,
                    instrument_id=instrument_id,
                )
                order_status_command = GenerateOrderStatusReports(
                    instrument_id=instrument_id,
                    start=since,
                    end=None,
                    open_only=use_open_orders_only,
                    command_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                )
                fill_reports_command = GenerateFillReports(
                    instrument_id=instrument_id,
                    venue_order_id=None,
                    start=since,
                    end=None,
                    command_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                )
                position_status_command = GeneratePositionStatusReports(
                    instrument_id=instrument_id,
                    start=since,
                    end=None,
                    command_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                )

                reports = await asyncio.gather(
                    client.generate_order_status_reports(order_status_command),
                    client.generate_fill_reports(fill_reports_command),
                    client.generate_position_status_reports(position_status_command),
                )
                targeted_order_reports = (
                    await self._generate_startup_targeted_order_status_reports(
                        client=client,
                        instrument_id=instrument_id,
                    )
                    if use_open_orders_only
                    else []
                )
                position_reports = self._normalize_startup_position_reports_for_instrument(
                    client=client,
                    instrument_id=instrument_id,
                    reports=cast("list[PositionStatusReport]", reports[2]),
                    log_prefix="Startup position report normalization",
                    ts_ns=self._clock.timestamp_ns(),
                )

                mass_status.add_order_reports(
                    reports=self._merge_startup_order_status_reports(
                        bulk_reports=cast("list[OrderStatusReport]", reports[0]),
                        targeted_reports=targeted_order_reports,
                    ),
                )
                mass_status.add_fill_reports(reports=reports[1])
                mass_status.add_position_reports(reports=position_reports)
        except Exception as e:
            self._log.exception("Cannot reconcile scoped startup execution state", e)
            return None

        return mass_status

    def _startup_order_status_should_use_open_only(
        self,
        client: ExecutionClient,
        instrument_id: InstrumentId,
    ) -> bool:
        snapshot = self._startup_snapshot_for_instrument(client.account_id, instrument_id)
        if snapshot.has_open_positions:
            self._log.info(
                f"Startup reconciliation snapshot has restored positions for {instrument_id}; "
                f"requesting only open venue orders and reconciling "
                f"{snapshot.total_open_order_count} cached open order(s) via targeted queries",
                LogColor.BLUE,
            )
            return True

        use_open_only = snapshot.total_cached_order_count == 0

        if use_open_only:
            self._log.info(
                f"Startup reconciliation snapshot is empty for {instrument_id}; "
                "requesting only open venue orders and using fills/positions for "
                "historical reconstruction",
                LogColor.BLUE,
            )

        return use_open_only

    def _normalize_netting_position_reports(
        self,
        reports: list[PositionStatusReport],
        log_prefix: str,
    ) -> list[PositionStatusReport]:
        collapsed_reports, collapse_events = collapse_duplicate_netting_position_reports(reports)
        for collapse_event in collapse_events:
            self._log.info(
                f"{log_prefix}: collapsed {collapse_event['report_count']} netting PositionStatusReports for "
                f"{collapse_event['instrument_id']} to ts_last={collapse_event['selected_ts_last']}, "
                f"signed_qty={collapse_event['selected_signed_qty']}, "
                f"discarded_flat_duplicates={collapse_event['discarded_flat_duplicates']}",
                LogColor.BLUE,
            )
        return collapsed_reports

    def _startup_synthetic_flat_position_report(
        self,
        *,
        client: ExecutionClient,
        instrument_id: InstrumentId,
        ts_ns: int,
        log_prefix: str,
    ) -> PositionStatusReport | None:
        if client.oms_type != OmsType.NETTING:
            return None

        instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            instrument_provider = getattr(client, "_instrument_provider", None)
            if instrument_provider is not None:
                instrument = instrument_provider.find(instrument_id)

        if instrument is None:
            self._log.warning(
                f"{log_prefix}: position query returned no reports for {instrument_id}, "
                "but the instrument is unavailable so a synthetic flat report cannot be created",
            )
            return None

        self._log.info(
            f"{log_prefix}: position query returned no reports for {instrument_id}; "
            "synthesizing FLAT PositionStatusReport for NETTING reconciliation",
            LogColor.BLUE,
        )
        return PositionStatusReport(
            account_id=client.account_id,
            instrument_id=instrument_id,
            position_side=PositionSide.FLAT,
            quantity=Quantity.zero(instrument.size_precision),
            report_id=UUID4(),
            ts_last=ts_ns,
            ts_init=ts_ns,
        )

    def _normalize_startup_position_reports_for_instrument(
        self,
        *,
        client: ExecutionClient,
        instrument_id: InstrumentId,
        reports: list[PositionStatusReport],
        log_prefix: str,
        ts_ns: int,
    ) -> list[PositionStatusReport]:
        normalized_reports = self._normalize_netting_position_reports(
            reports=reports,
            log_prefix=log_prefix,
        )
        if normalized_reports:
            return normalized_reports

        synthetic_report = self._startup_synthetic_flat_position_report(
            client=client,
            instrument_id=instrument_id,
            ts_ns=ts_ns,
            log_prefix=log_prefix,
        )
        return [synthetic_report] if synthetic_report is not None else []

    async def reconcile_execution_state(
        self,
        timeout_secs: float = 10.0,
    ) -> bool:
        """
        Reconcile execution state as main entry point for startup reconciliation,
        coordinating reconciliation across all execution clients.
        """
        PyCondition.positive(timeout_secs, "timeout_secs")
        deadline = self._loop.time() + timeout_secs

        try:
            self._capture_startup_reconciliation_snapshot()

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
                self._generate_startup_mass_status(c, reconciliation_lookback_mins)
                for c in self._clients.values()
            ]
            mass_status_timeout = deadline - self._loop.time()
            if mass_status_timeout <= 0:
                self._log.error("Execution reconciliation timed out before mass-status generation started")
                return False
            try:
                mass_status_all = await asyncio.wait_for(
                    asyncio.gather(*mass_status_coros, return_exceptions=True),
                    timeout=mass_status_timeout,
                )
            except asyncio.TimeoutError:
                self._log.error(
                    f"Execution reconciliation timed out after {timeout_secs}s while awaiting mass status reports",
                )
                return False

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
                # venue = mass_status.venue
                result = self._reconcile_execution_mass_status(
                    mass_status,
                    allow_startup_external_cleanup=True,
                )

                if not result and self.filter_position_reports:
                    self._log_reconciliation_result(client_id, result)
                    results.append(result)
                    self._log.warning(
                        "`filter_position_reports` enabled, skipping further reconciliation",
                    )
                    continue

                client = self._clients[client_id]

                # Check internal and external position reconciliation
                pending_position_commands: dict[InstrumentId, GeneratePositionStatusReports] = {}

                # For routing brokers, venue may differ from instrument venue (e.g., IB client venue
                # vs NYSE instrument venue), so filter by account_id instead of venue
                for position in self._cache.positions_open(
                    venue=None,
                    account_id=client.account_id,
                ):
                    instrument_id = position.instrument_id
                    if not self._should_reconcile_instrument_for_client(
                        client=client,
                        instrument_id=instrument_id,
                    ):
                        continue
                    if instrument_id in mass_status.position_reports:
                        self._log.debug(
                            f"Position {instrument_id} for {client_id} already reconciled",
                        )
                        continue  # Already reconciled

                    if instrument_id in pending_position_commands:
                        self._log.debug(
                            f"Position {instrument_id} for {client_id} already queued for "
                            "startup follow-up reconciliation",
                        )
                        continue

                    self._log.info(f"{position} pending reconciliation")
                    pending_position_commands[instrument_id] = GeneratePositionStatusReports(
                        instrument_id=instrument_id,
                        start=None,
                        end=None,
                        command_id=UUID4(),
                        ts_init=self._clock.timestamp_ns(),
                    )
                report_tasks = [
                    (
                        instrument_id,
                        client.generate_position_status_reports(position_status_command),
                    )
                    for instrument_id, position_status_command in pending_position_commands.items()
                ]

                if report_tasks:
                    # Reconcile specific internal open positions
                    self._log.info(f"Awaiting {len(report_tasks)} position reports for {client_id}")

                    position_results: list[bool] = []
                    position_reports_timeout = deadline - self._loop.time()
                    if position_reports_timeout <= 0:
                        self._log.error(
                            f"Execution reconciliation timed out after {timeout_secs}s before awaiting position reports for {client_id}",
                        )
                        return False
                    try:
                        position_report_results = await asyncio.wait_for(
                            asyncio.gather(
                                *(task for _, task in report_tasks),
                                return_exceptions=True,
                            ),
                            timeout=position_reports_timeout,
                        )
                    except asyncio.TimeoutError:
                        self._log.error(
                            f"Execution reconciliation timed out after {timeout_secs}s while awaiting position reports for {client_id}",
                        )
                        return False

                    for (instrument_id, _), task_result_or_exception in zip(
                        report_tasks,
                        position_report_results,
                        strict=False,
                    ):
                        if isinstance(task_result_or_exception, Exception):
                            self._log.error(
                                f"Failed to generate position status reports: {task_result_or_exception}",
                            )
                            position_results.append(False)
                            continue

                        task_result = cast("list[PositionStatusReport]", task_result_or_exception)
                        task_result = self._normalize_startup_position_reports_for_instrument(
                            client=client,
                            instrument_id=instrument_id,
                            reports=task_result,
                            log_prefix="Startup follow-up position query",
                            ts_ns=self._clock.timestamp_ns(),
                        )
                        for report in task_result:
                            position_result = self._reconcile_position_report(
                                report,
                                allow_startup_external_cleanup=True,
                            )
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
            self._clear_startup_reconciliation_snapshot()
            # Always signal completion to prevent continuous loop signal await hang
            self._startup_reconciliation_event.set()

    def _client_routed_venues(self, client: ExecutionClient) -> set[Venue]:
        routed_venues = {
            venue for venue, mapped_client in self._routing_map.items() if mapped_client is client
        }
        if client.venue is not None:
            routed_venues.add(client.venue)
        return routed_venues

    def _client_scoped_reconciliation_instrument_ids(
        self,
        client: ExecutionClient,
    ) -> list[InstrumentId]:
        if not self.reconciliation_instrument_ids:
            return []
        routed_venues = self._client_routed_venues(client)
        return [
            instrument_id
            for instrument_id in self.reconciliation_instrument_ids
            if instrument_id.venue in routed_venues
        ]

    def _should_reconcile_instrument_for_client(
        self,
        *,
        client: ExecutionClient,
        instrument_id: InstrumentId,
    ) -> bool:
        if not self.reconciliation_instrument_ids:
            return True
        scoped_ids = self._client_scoped_reconciliation_instrument_ids(client)
        return instrument_id in scoped_ids

    async def _generate_reconciliation_mass_status_for_client(
        self,
        *,
        client: ExecutionClient,
        lookback_mins: int | None,
    ) -> ExecutionMassStatus | None:
        scoped_ids = self._client_scoped_reconciliation_instrument_ids(client)
        if not self.reconciliation_instrument_ids:
            return await client.generate_mass_status(lookback_mins)

        mass_status = ExecutionMassStatus(
            client_id=client.id,
            account_id=client.account_id,
            venue=client.venue,
            report_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        if not scoped_ids:
            return mass_status

        since: pd.Timestamp | None = None
        if lookback_mins is not None:
            since = self._clock.utc_now() - pd.Timedelta(minutes=lookback_mins)

        order_status_coros = [
            client.generate_order_status_reports(
                GenerateOrderStatusReports(
                    instrument_id=instrument_id,
                    start=since,
                    end=None,
                    open_only=False,
                    command_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                ),
            )
            for instrument_id in scoped_ids
        ]
        fill_report_coros = [
            client.generate_fill_reports(
                GenerateFillReports(
                    instrument_id=instrument_id,
                    venue_order_id=None,
                    start=since,
                    end=None,
                    command_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                ),
            )
            for instrument_id in scoped_ids
        ]
        position_report_coros = [
            client.generate_position_status_reports(
                GeneratePositionStatusReports(
                    instrument_id=instrument_id,
                    start=since,
                    end=None,
                    command_id=UUID4(),
                    ts_init=self._clock.timestamp_ns(),
                ),
            )
            for instrument_id in scoped_ids
        ]

        try:
            order_results, fill_results, position_results = await asyncio.gather(
                asyncio.gather(*order_status_coros),
                asyncio.gather(*fill_report_coros),
                asyncio.gather(*position_report_coros),
            )
        except Exception as e:
            self._log.error(
                f"Failed to generate scoped execution mass status for {client.id}: {e}",
            )
            return None

        for reports in order_results:
            mass_status.add_order_reports(reports=reports)
        for reports in fill_results:
            mass_status.add_fill_reports(reports=reports)
        for reports in position_results:
            mass_status.add_position_reports(reports=reports)
        return mass_status

    def _log_reconciliation_result(self, value: ClientId | InstrumentId, result: bool) -> None:
        if result:
            self._log.info(f"Reconciliation for {value} succeeded", LogColor.GREEN)
        else:
            self._log.warning(f"Reconciliation for {value} failed")

    def reconcile_execution_report(self, report: ExecutionReport) -> bool:
        """
        Reconcile a single execution report received at runtime, routing to appropriate
        reconciliation method based on report type.
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
            result = self._reconcile_position_report(
                report,
                allow_startup_external_cleanup=False,
            )
        else:
            self._log.error(  # pragma: no cover (design-time error)
                f"Cannot handle unrecognized report: {report}",  # pragma: no cover (design-time error)
            )
            return False

        self._msgbus.publish(
            topic=f"reports.execution.{report.instrument_id.venue}.{report.instrument_id.symbol}",
            msg=report,
        )

        return result

    # -- RECONCILIATION ----------------------------------------------------------------------------

    def reconcile_execution_mass_status(self, report: ExecutionMassStatus) -> None:
        """
        Entry point for mass status reconciliation.
        """
        self._reconcile_execution_mass_status(
            report,
            allow_startup_external_cleanup=False,
        )

    def _reconcile_execution_mass_status(
        self,
        mass_status: ExecutionMassStatus,
        allow_startup_external_cleanup: bool = False,
    ) -> bool:
        self._log.debug(f"<--[RPT] {mass_status}")
        self.report_count += 1

        self._log.info(
            f"Reconciling ExecutionMassStatus for {mass_status.venue}",
            color=LogColor.BLUE,
        )

        # Adjust fills for instruments with incomplete first lifecycles
        self._adjust_mass_status_fills(
            mass_status,
            allow_startup_external_cleanup=allow_startup_external_cleanup,
        )

        # Deduplicate orders in mass status
        self._deduplicate_mass_status_orders(mass_status)

        results: list[bool] = []
        reconciled_orders: set[ClientOrderId] = set()
        reconciled_trades: set[TradeId] = set()

        # Reconcile all reported orders
        for venue_order_id, order_report in mass_status.order_reports.items():
            trades = mass_status.fill_reports.get(venue_order_id, [])

            if not self._consider_for_reconciliation(order_report.instrument_id):
                self._log_skipping_reconciliation_on_instrument_id(order_report)
                continue

            client_order_id = order_report.client_order_id

            if client_order_id is not None and client_order_id in self.filtered_client_order_ids:
                self._log.debug(
                    f"Skipping {type(order_report).__name__} reconciliation for {order_report.client_order_id!r}: "
                    f"in `filtered_client_order_ids` list",
                    LogColor.MAGENTA,
                )
                continue

            # Check for duplicate trade IDs
            for fill_report in trades:
                if fill_report.trade_id in reconciled_trades:
                    self._log.warning(
                        f"Duplicate {fill_report.trade_id!r} detected: {fill_report}",
                    )

                reconciled_trades.add(fill_report.trade_id)

            try:
                # Apply all fills - let position cycle naturally through all lifecycles
                result = self._reconcile_order_report(order_report, trades)
            except InvalidStateTrigger as e:
                self._log.error(str(e))
                result = False

            results.append(result)

            if order_report.client_order_id is not None:
                # Only track orders where instrument was loaded (others are filtered)
                instrument = self._cache.instrument(order_report.instrument_id)
                if instrument is not None:
                    reconciled_orders.add(order_report.client_order_id)

                    if result and order_report.venue_order_id is not None:
                        self._ensure_venue_order_id_indexed(
                            client_order_id=order_report.client_order_id,
                            venue_order_id=order_report.venue_order_id,
                        )

        if not self.filter_position_reports:
            position_reports: list[PositionStatusReport]

            # Reconcile all reported positions
            for position_reports in mass_status.position_reports.values():
                for report in position_reports:
                    if not self._consider_for_reconciliation(report.instrument_id):
                        self._log_skipping_reconciliation_on_instrument_id(report)
                        continue

                    result = self._reconcile_position_report(
                        report,
                        allow_startup_external_cleanup=allow_startup_external_cleanup,
                    )
                    results.append(result)

        # Publish mass status
        self._msgbus.publish(
            topic=f"reports.execution.{mass_status.venue}",
            msg=mass_status,
        )

        # Validate reconciliation state for consistency
        self._validate_reconciliation_state(mass_status, reconciled_orders)

        return all(results)

    def _adjust_mass_status_fills(
        self,
        mass_status: ExecutionMassStatus,
        allow_startup_external_cleanup: bool = False,
    ) -> None:
        # Adjust fills for instruments with incomplete first lifecycles
        # Start with original orders and fills
        final_orders = dict(mass_status._order_reports)
        final_fills = dict(mass_status._fill_reports)

        reconciliation_instruments: list[Instrument] = []
        for instrument_id, position_reports in mass_status.position_reports.items():
            # Skip hedge mode instruments (have venue_position_id) as partial-window
            # adjustment assumes a single net position per instrument
            is_hedge_mode = any(r.venue_position_id is not None for r in position_reports)
            if is_hedge_mode:
                self._log.debug(
                    f"Skipping fill adjustment for {instrument_id}: "
                    f"hedge mode (has venue_position_id)",
                )
                continue

            # Respect reconciliation_instrument_ids filter
            if not self._consider_for_reconciliation(instrument_id):
                self._log.debug(
                    f"Skipping fill adjustment for {instrument_id}: "
                    f"not in `reconciliation_instrument_ids` include list",
                )
                continue

            instrument = self._cache.instrument(instrument_id)
            if not instrument:
                self._log.debug(
                    f"Skipping fill adjustment for {instrument_id}: instrument not found in cache",
                )
                continue

            startup_snapshot = self._startup_snapshot_for_instrument(
                position_reports[0].account_id if position_reports else None,
                instrument_id,
            )
            if allow_startup_external_cleanup and startup_snapshot.has_open_positions:
                self._log.info(
                    f"Skipping fill adjustment for {instrument_id}: startup snapshot already "
                    "contains restored open positions, so replaying partial-window lifecycle "
                    "history would double-apply fills",
                    LogColor.BLUE,
                )
                continue

            reconciliation_instruments.append(instrument)

        self._log.info(
            f"Attempting to adjust fills for {len(reconciliation_instruments)} instruments",
            LogColor.BLUE,
        )
        adjusted_results = adjust_fills_for_partial_window(
            mass_status,
            reconciliation_instruments,
            self._log,
        )
        self._log.info(
            f"Updating adjusted fills for {len(reconciliation_instruments)} instruments",
            LogColor.BLUE,
        )

        for instrument_id, (
            adjusted_orders_for_instrument,
            adjusted_fills_for_instrument,
        ) in adjusted_results.items():
            # Remove old orders and fills for this instrument
            for venue_order_id in list(final_orders.keys()):
                order = final_orders[venue_order_id]
                if order.instrument_id == instrument_id:
                    del final_orders[venue_order_id]

            for venue_order_id in list(final_fills.keys()):
                fills = final_fills[venue_order_id]
                if fills and fills[0].instrument_id == instrument_id:
                    del final_fills[venue_order_id]

            # Add adjusted orders and fills for this instrument
            final_orders.update(adjusted_orders_for_instrument)
            final_fills.update(adjusted_fills_for_instrument)

        # Apply all adjustments at once
        mass_status._order_reports = final_orders
        mass_status._fill_reports = final_fills
        self._log.info(
            f"Final order_reports contains {len(final_orders)} orders, fill_reports contains {len(final_fills)} fills across all instruments",
            LogColor.BLUE,
        )

    def _deduplicate_mass_status_orders(self, mass_status: ExecutionMassStatus) -> None:
        # Remove duplicate orders within mass status report
        seen_client_order_ids: dict[ClientOrderId, VenueOrderId] = {}
        duplicate_venue_order_ids: list[VenueOrderId] = []
        orders_to_skip: list[VenueOrderId] = []

        # First pass: deduplicate within the current report
        for venue_order_id, order_report in mass_status._order_reports.items():
            if order_report.client_order_id is not None:
                if order_report.client_order_id in seen_client_order_ids:
                    # Duplicate found in current report - mark for removal
                    duplicate_venue_order_ids.append(venue_order_id)
                    self._log.warning(
                        f"Deduplicating order: {order_report.client_order_id} "
                        f"(venue_order_id={venue_order_id}, "
                        f"keeping first occurrence {seen_client_order_ids[order_report.client_order_id]})",
                    )
                else:
                    # First occurrence - track it
                    seen_client_order_ids[order_report.client_order_id] = venue_order_id

        # Second pass: check against cached orders to prevent duplicates
        # Only skip if order is an exact match (same status, filled_qty, etc.)
        # This prevents duplicate creation while still allowing reconciliation of mismatches
        for venue_order_id, order_report in mass_status._order_reports.items():
            if venue_order_id in duplicate_venue_order_ids:
                continue  # Already marked as duplicate

            # Check if this order already exists in cache by client_order_id
            if order_report.client_order_id is not None:
                cached_order = self._cache.order(order_report.client_order_id)
                if cached_order is not None:
                    # Skip closed reconciliation orders to prevent duplicate inferred fills on restart
                    if (
                        cached_order.is_closed
                        and cached_order.tags is not None
                        and "RECONCILIATION" in cached_order.tags
                    ):
                        orders_to_skip.append(venue_order_id)
                        self._log.debug(
                            f"Skipping closed reconciliation order {order_report.client_order_id}: "
                            f"synthetic position adjustment from previous session",
                        )
                        continue

                    # Order exists in cache - check if it's an exact duplicate
                    # Only skip if it's an exact match (prevents duplicate creation)
                    # But still reconcile if there are any discrepancies
                    report_filled = (
                        order_report.filled_qty
                        if order_report.filled_qty is not None
                        else Quantity.zero(cached_order.quantity.precision)
                    )

                    # Check for exact match - same status, filled_qty, and instrument
                    is_exact_match = (
                        cached_order.status == order_report.order_status
                        and cached_order.filled_qty == report_filled
                        and cached_order.instrument_id == order_report.instrument_id
                        and cached_order.side == order_report.order_side
                    )

                    if is_exact_match:
                        # Exact duplicate - skip to prevent duplicate creation
                        orders_to_skip.append(venue_order_id)
                        self._log.debug(
                            f"Skipping exact duplicate order {order_report.client_order_id}: "
                            f"order already exists in cache with identical state",
                        )
                        continue
                    # If not exact match, continue with reconciliation to fix discrepancies

            # Also check by venue_order_id if client_order_id lookup failed or wasn't provided
            if order_report.venue_order_id is not None and order_report.client_order_id is None:
                cached_client_id = self._cache.client_order_id(order_report.venue_order_id)
                if cached_client_id is not None:
                    cached_order = self._cache.order(cached_client_id)
                    if cached_order is not None:
                        # Update the report to use the cached client_order_id for consistency
                        order_report.client_order_id = cached_client_id
                        self._log.debug(
                            f"Found cached order {cached_client_id} by venue_order_id {order_report.venue_order_id}, "
                            f"updating report to use cached client_order_id",
                        )
                        # Don't skip - still need to reconcile in case there are discrepancies

        # Remove duplicates and orders to skip
        orders_to_remove = set(duplicate_venue_order_ids) | set(orders_to_skip)
        for venue_order_id in orders_to_remove:
            del mass_status._order_reports[venue_order_id]

            # Also remove associated fills
            if venue_order_id in mass_status._fill_reports:
                del mass_status._fill_reports[venue_order_id]

        if orders_to_remove:
            self._log.debug(
                f"Removed {len(orders_to_remove)} duplicate/skipped order(s) from reconciliation "
                f"({len(duplicate_venue_order_ids)} duplicates, {len(orders_to_skip)} already in cache)",
                LogColor.YELLOW,
            )

    def _validate_reconciliation_state(
        self,
        mass_status: ExecutionMassStatus,
        reconciled_orders: set[ClientOrderId],
    ) -> None:
        venue_order_ids_seen: set[VenueOrderId] = set()
        issues: list[str] = []

        for order_report in mass_status._order_reports.values():
            if order_report.venue_order_id is None:
                continue

            # Skip orders that were filtered (e.g., instrument not loaded)
            if order_report.client_order_id not in reconciled_orders:
                self._log.debug(
                    f"Skipping validation for {order_report.client_order_id} "
                    f"(venue_order_id={order_report.venue_order_id}) - not in reconciled_orders",
                )
                continue

            if order_report.venue_order_id in venue_order_ids_seen:
                issues.append(
                    f"Duplicate venue_order_id {order_report.venue_order_id} in mass status",
                )

            venue_order_ids_seen.add(order_report.venue_order_id)

            # Check if venue_order_id is properly indexed
            if order_report.client_order_id:
                cached_client_id = self._cache.client_order_id(order_report.venue_order_id)
                if cached_client_id is None:
                    issues.append(
                        f"Venue order ID {order_report.venue_order_id} not indexed in cache "
                        f"for client_order_id {order_report.client_order_id}",
                    )
                elif cached_client_id != order_report.client_order_id:
                    issues.append(
                        f"Venue order ID {order_report.venue_order_id} indexing mismatch: "
                        f"expected {order_report.client_order_id}, found {cached_client_id}",
                    )

        if issues:
            self._log.warning(
                f"Reconciliation state validation found {len(issues)} issue(s):\n"
                + "\n".join(f"  - {issue}" for issue in issues),
            )
        else:
            self._log.debug(
                f"Reconciliation state validation passed for {len(mass_status._order_reports)} order(s)",
            )

    # -- FILL RECONCILIATION -----------------------------------------------------------------------

    def _reconcile_fill_report_single(self, report: FillReport) -> bool:
        if self._is_shutting_down:
            return True  # Skip reconciliation during shutdown

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
            # Try to find order by venue_order_id if client_order_id lookup failed
            # This handles cases where external orders might not be fully indexed yet
            if report.venue_order_id is not None:
                order = self._find_order_by_venue_order_id(
                    venue_order_id=report.venue_order_id,
                    instrument_id=report.instrument_id,
                    order_side=None,  # Don't filter by side to find any matching order
                )
                if order is not None:
                    self._log.debug(
                        f"Found order {order.client_order_id} by venue_order_id "
                        f"{report.venue_order_id} for fill report",
                    )
                    # Ensure mapping is indexed
                    self._ensure_venue_order_id_indexed(
                        client_order_id=order.client_order_id,
                        venue_order_id=report.venue_order_id,
                        log_context="for fill report",
                    )

            if order is None:
                self._log.warning(
                    f"FillReport received before order cached for {client_order_id!r} "
                    f"(venue_order_id={report.venue_order_id!r}), deferring reconciliation",
                )
                return False  # Failed

        # Log external order processing for better visibility
        if order.strategy_id.value == "EXTERNAL":
            self._log.debug(
                f"Processing fill for external order {order.client_order_id} "
                f"(venue_order_id={order.venue_order_id})",
            )

        instrument: Instrument | None = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.debug(
                f"Cannot reconcile order for {order.client_order_id!r}: "
                f"instrument {order.instrument_id} not found",
            )
            return True  # Filtered instrument not loaded

        return self._reconcile_fill_report(order, report, instrument)

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

    def _rollback_fill_audit_entry(
        self,
        client_order_id: ClientOrderId,
        audit_entry: tuple[TradeId, str, int],
    ) -> None:
        # Remove audit entry when fill application fails
        if audit_entry in self._fill_application_audit.get(client_order_id, []):
            self._fill_application_audit[client_order_id].remove(audit_entry)

    def _create_order_status_report_from_cached_order(
        self,
        cached_order: Order,
        instrument_id: InstrumentId,
        account_id: AccountId,
        order_side: OrderSide,
        quantity: Quantity,
        filled_qty: Quantity,
        price: Price | None,
        avg_px: Decimal | None,
        ts_now: int,
        venue_position_id: PositionId | None = None,
    ) -> OrderStatusReport:
        return OrderStatusReport(
            instrument_id=instrument_id,
            account_id=account_id,
            venue_order_id=cached_order.venue_order_id or VenueOrderId(str(UUID4())),
            venue_position_id=venue_position_id,
            order_side=order_side,
            order_type=cached_order.order_type,
            time_in_force=cached_order.time_in_force,
            order_status=OrderStatus.FILLED,
            price=price,
            quantity=quantity,
            filled_qty=filled_qty,
            avg_px=avg_px,
            report_id=UUID4(),
            ts_accepted=ts_now,
            ts_last=ts_now,
            ts_init=ts_now,
            client_order_id=cached_order.client_order_id,
        )

    # -- POSITION RECONCILIATION -------------------------------------------------------------------

    def _reconcile_position_report(
        self,
        report: PositionStatusReport,
        allow_startup_external_cleanup: bool = False,
    ) -> bool:
        if self._is_shutting_down:
            return True  # Skip reconciliation during shutdown

        if not self._consider_for_reconciliation(report.instrument_id):
            self._log_skipping_reconciliation_on_instrument_id(report)
            return True  # Filtered

        if report.venue_position_id is not None:
            return self._reconcile_position_report_hedging(report)
        else:
            return self._reconcile_position_report_netting(
                report,
                allow_startup_external_cleanup=allow_startup_external_cleanup,
            )

    def _consider_for_reconciliation(self, instrument_id: InstrumentId) -> bool:
        if self.reconciliation_instrument_ids:
            return instrument_id in self.reconciliation_instrument_ids

        return True

    def _log_skipping_reconciliation_on_instrument_id(self, report: ExecutionReport) -> None:
        self._log.debug(
            f"Skipping {type(report).__name__} reconciliation for {report.instrument_id}: "
            f"not in `reconciliation_instrument_ids` include list",
            LogColor.MAGENTA,
        )

    def _reconcile_position_report_hedging(self, report: PositionStatusReport) -> bool:
        self._log.info(
            f"Reconciling HEDGE position for {report.instrument_id}, venue_position_id={report.venue_position_id}",
            LogColor.BLUE,
        )

        position: Position | None = self._cache.position(report.venue_position_id)

        if position is None:
            if report.signed_decimal_qty == 0:
                return True  # Both flat, no issue

            if not self.generate_missing_orders:
                self._log.error(
                    f"Cannot reconcile position: {report.venue_position_id!r} not found "
                    "and `generate_missing_orders` is disabled",
                )
                return False

            return self._reconcile_missing_hedge_position(report)

        position_signed_decimal_qty: Decimal = position.signed_decimal_qty()

        if position_signed_decimal_qty != report.signed_decimal_qty:
            if not self.generate_missing_orders:
                self._log.error(
                    f"Cannot reconcile {report.instrument_id} {report.venue_position_id!r}: "
                    f"position net qty {position_signed_decimal_qty} != reported net qty "
                    f"{report.signed_decimal_qty} and `generate_missing_orders` is disabled",
                )
                return False

            return self._reconcile_hedge_position_discrepancy(
                report=report,
                position=position,
                position_signed_decimal_qty=position_signed_decimal_qty,
            )

        return True  # Reconciled

    def _reconcile_hedge_position_discrepancy(
        self,
        report: PositionStatusReport,
        position: Position,
        position_signed_decimal_qty: Decimal,
    ) -> bool:
        instrument = self._cache.instrument(report.instrument_id)
        if instrument is None:
            self._log.debug(
                f"Cannot reconcile position for {report.instrument_id}: instrument not found",
            )
            return True  # Filtered instrument not loaded

        diff = abs(position_signed_decimal_qty - report.signed_decimal_qty)
        diff_quantity = Quantity(diff, instrument.size_precision)

        if diff_quantity == 0:
            self._log.debug(
                f"Difference quantity rounds to zero for {instrument.id}, skipping",
            )
            return True

        self._log.warning(
            f"Hedge position discrepancy for {report.instrument_id} "
            f"{report.venue_position_id!r}: cached={position_signed_decimal_qty}, "
            f"venue={report.signed_decimal_qty}, generating reconciliation order",
            LogColor.YELLOW,
        )

        current_avg_px = Decimal(str(position.avg_px_open)) if position.avg_px_open else None

        diff_report = self._create_position_reconciliation_report(
            report=report,
            instrument=instrument,
            position_signed_decimal_qty=position_signed_decimal_qty,
            diff_quantity=diff_quantity,
            current_avg_px=current_avg_px,
        )

        if diff_report:
            self._reconcile_order_report(diff_report, trades=[], is_external=False)

        return True

    def _reconcile_missing_hedge_position(self, report: PositionStatusReport) -> bool:
        instrument = self._cache.instrument(report.instrument_id)
        if instrument is None:
            self._log.debug(
                f"Cannot reconcile position for {report.instrument_id}: instrument not found",
            )
            return True  # Filtered instrument not loaded

        quantity = Quantity(abs(report.signed_decimal_qty), instrument.size_precision)

        if quantity == 0:
            return True

        self._log.warning(
            f"Missing hedge position for {report.instrument_id} "
            f"{report.venue_position_id!r}: venue reports {report.signed_decimal_qty}, "
            f"generating reconciliation order",
            LogColor.YELLOW,
        )

        diff_report = self._create_position_reconciliation_report(
            report=report,
            instrument=instrument,
            position_signed_decimal_qty=Decimal(0),
            diff_quantity=quantity,
            current_avg_px=None,
        )

        if diff_report:
            self._reconcile_order_report(diff_report, trades=[], is_external=False)

        return True

    @staticmethod
    def _sum_position_signed_decimal_qty(positions: list[Position]) -> Decimal:
        total = Decimal()
        for position in positions:
            total += position.signed_decimal_qty()

        return total

    @staticmethod
    def _calculate_weighted_avg_px(positions: list[Position]) -> Decimal | None:
        if not positions:
            return None

        total_value = Decimal(0)
        total_qty = Decimal(0)
        for position in positions:
            qty = abs(position.signed_decimal_qty())
            if position.avg_px_open and qty > 0:
                total_value += Decimal(str(position.avg_px_open)) * qty
                total_qty += qty

        if total_qty == 0:
            return None

        return total_value / total_qty

    def _effective_netting_positions_for_venue_qty(
        self,
        positions_open: list[Position],
        instrument_id: InstrumentId,
        venue_qty: Decimal | None,
    ) -> tuple[list[Position], list[Position], Decimal, Decimal]:
        raw_qty = self._sum_position_signed_decimal_qty(positions_open)

        if venue_qty is None or raw_qty == venue_qty or not positions_open:
            return positions_open, [], raw_qty, raw_qty

        effective_positions: list[Position] = []
        artifact_positions: list[Position] = []
        for position in positions_open:
            if is_external_reconciliation_artifact_position(
                position,
                order_lookup=self._cache.orders_for_position,
            ):
                artifact_positions.append(position)
            else:
                effective_positions.append(position)

        if not artifact_positions:
            return positions_open, [], raw_qty, raw_qty

        effective_qty = self._sum_position_signed_decimal_qty(effective_positions)
        if effective_qty != venue_qty:
            return positions_open, [], raw_qty, raw_qty

        return effective_positions, artifact_positions, effective_qty, raw_qty

    def _startup_effective_netting_positions_for_venue_qty(
        self,
        positions_open: list[Position],
        account_id: AccountId | None,
        instrument_id: InstrumentId,
        venue_qty: Decimal | None,
    ) -> tuple[list[Position], list[Position], Decimal, Decimal]:
        effective_positions, artifact_positions, effective_qty, raw_qty = (
            self._effective_netting_positions_for_venue_qty(
                positions_open=positions_open,
                instrument_id=instrument_id,
                venue_qty=venue_qty,
            )
        )

        if (
            venue_qty is None
            or raw_qty == venue_qty
            or not positions_open
            or artifact_positions
        ):
            return effective_positions, artifact_positions, effective_qty, raw_qty

        snapshot = self._startup_snapshot_for_instrument(account_id, instrument_id)
        startup_external_position_ids = set(snapshot.startup_external_position_ids)
        if not startup_external_position_ids:
            return positions_open, [], raw_qty, raw_qty

        external_positions = [
            position for position in positions_open if position.id in startup_external_position_ids
        ]
        if not external_positions:
            return positions_open, [], raw_qty, raw_qty

        effective_positions = [
            position for position in positions_open if position.id not in startup_external_position_ids
        ]
        effective_qty = self._sum_position_signed_decimal_qty(effective_positions)
        if snapshot.startup_non_external_position_qty != venue_qty or effective_qty != venue_qty:
            return positions_open, [], raw_qty, raw_qty

        self._log.info(
            f"Treating EXTERNAL netting positions as stale startup reconciliation artifacts for "
            f"{instrument_id}: raw_qty={raw_qty}, effective_qty={effective_qty}, "
            f"external_qty={self._sum_position_signed_decimal_qty(external_positions)}, "
            f"snapshot_position_ids={[position.id.value for position in external_positions]}",
            LogColor.BLUE,
        )
        return effective_positions, external_positions, effective_qty, raw_qty

    def _cleanup_stale_external_reconciliation_positions(
        self,
        report: PositionStatusReport,
        instrument: Instrument,
        artifact_positions: list[Position],
        raw_qty: Decimal,
        effective_qty: Decimal,
        publish_startup_alert: bool = False,
    ) -> bool:
        stale_position_ids = [position.id.value for position in artifact_positions]
        self._log.info(
            f"Closing stale EXTERNAL reconciliation positions for {report.instrument_id}: "
            f"raw_qty={raw_qty}, effective_qty={effective_qty}, "
            f"stale_qty={self._sum_position_signed_decimal_qty(artifact_positions)}, "
            f"position_ids={stale_position_ids}",
            LogColor.BLUE,
        )

        if publish_startup_alert:
            self._publish_startup_position_reconciliation_alert(
                report=report,
                message=(
                    f"Startup reconciliation removed stale EXTERNAL cached positions for "
                    f"{report.instrument_id}"
                ),
                cached_qty=raw_qty,
                venue_qty=report.signed_decimal_qty,
                position_ids=stale_position_ids,
                raw_qty=raw_qty,
                effective_qty=effective_qty,
            )

        for position in artifact_positions:
            position_qty = position.signed_decimal_qty()
            diff_quantity = Quantity(abs(position_qty), instrument.size_precision)
            if diff_quantity == 0:
                continue

            cleanup_report = PositionStatusReport(
                account_id=report.account_id,
                instrument_id=report.instrument_id,
                venue_position_id=position.id,
                position_side=PositionSide.FLAT,
                quantity=Quantity.zero(),
                report_id=UUID4(),
                ts_last=report.ts_last,
                ts_init=report.ts_init,
            )
            current_avg_px = Decimal(str(position.avg_px_open)) if position.avg_px_open else None
            diff_report = self._create_position_reconciliation_report(
                report=cleanup_report,
                instrument=instrument,
                position_signed_decimal_qty=position_qty,
                diff_quantity=diff_quantity,
                current_avg_px=current_avg_px,
            )
            if diff_report is None:
                continue

            if not self._reconcile_order_report(diff_report, trades=[], is_external=False):
                return False

        return True

    def _reconcile_position_report_netting(
        self,
        report: PositionStatusReport,
        allow_startup_external_cleanup: bool = False,
    ) -> bool:
        self._log.info(f"Reconciling NET position for {report.instrument_id}", LogColor.BLUE)

        instrument = self._cache.instrument(report.instrument_id)
        if instrument is None:
            self._log.debug(
                f"Cannot reconcile position for {report.instrument_id}: instrument not found",
            )
            return True  # Filtered instrument not loaded

        positions_open_kwargs: dict[str, Any] = {
            "venue": None,  # Faster query filtering
            "instrument_id": report.instrument_id,
        }
        if report.account_id is not None:
            positions_open_kwargs["account_id"] = report.account_id

        positions_open: list[Position] = self._cache.positions_open(**positions_open_kwargs)

        if allow_startup_external_cleanup:
            effective_positions, artifact_positions, position_signed_decimal_qty, raw_position_signed_decimal_qty = (
                self._startup_effective_netting_positions_for_venue_qty(
                    positions_open=positions_open,
                    account_id=report.account_id,
                    instrument_id=report.instrument_id,
                    venue_qty=report.signed_decimal_qty,
                )
            )
        else:
            effective_positions, artifact_positions, position_signed_decimal_qty, raw_position_signed_decimal_qty = (
                self._effective_netting_positions_for_venue_qty(
                    positions_open=positions_open,
                    instrument_id=report.instrument_id,
                    venue_qty=report.signed_decimal_qty,
                )
            )

        self._log.info(f"{report.signed_decimal_qty=}", LogColor.BLUE)
        if raw_position_signed_decimal_qty != position_signed_decimal_qty:
            self._log.info(f"{raw_position_signed_decimal_qty=}", LogColor.BLUE)
        self._log.info(f"{position_signed_decimal_qty=}", LogColor.BLUE)

        if artifact_positions:
            return self._cleanup_stale_external_reconciliation_positions(
                report=report,
                instrument=instrument,
                artifact_positions=artifact_positions,
                raw_qty=raw_position_signed_decimal_qty,
                effective_qty=position_signed_decimal_qty,
                publish_startup_alert=allow_startup_external_cleanup,
            )

        # Check if quantities match
        quantities_match = position_signed_decimal_qty == report.signed_decimal_qty

        if not quantities_match:
            if allow_startup_external_cleanup and raw_position_signed_decimal_qty != 0:
                self._publish_startup_position_reconciliation_alert(
                    report=report,
                    message=(
                        f"Startup reconciliation detected stale cached position for "
                        f"{report.instrument_id}"
                    ),
                    cached_qty=raw_position_signed_decimal_qty,
                    venue_qty=report.signed_decimal_qty,
                )
            if not self.generate_missing_orders:
                self._log.error(
                    f"Cannot reconcile {report.instrument_id}: "
                    f"position net qty {position_signed_decimal_qty} != reported net qty "
                    f"{report.signed_decimal_qty} and `generate_missing_orders` is disabled",
                )
                return False

            diff = abs(position_signed_decimal_qty - report.signed_decimal_qty)
            diff_quantity = Quantity(diff, instrument.size_precision)
            self._log.info(f"{diff_quantity=}", LogColor.BLUE)

            if diff_quantity == 0:
                self._log.debug(
                    f"Difference quantity rounds to zero for {instrument.id}, skipping order generation",
                )
                return True

            # Calculate current position average price if available (needed for reconciliation)
            current_avg_px = self._calculate_weighted_avg_px(effective_positions)

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
                return self._reconcile_cross_zero_position(
                    report=report,
                    instrument=instrument,
                    position_signed_decimal_qty=position_signed_decimal_qty,
                    current_avg_px=current_avg_px,
                )

            diff_report = self._create_position_reconciliation_report(
                report=report,
                instrument=instrument,
                position_signed_decimal_qty=position_signed_decimal_qty,
                diff_quantity=diff_quantity,
                current_avg_px=current_avg_px,
            )
            if diff_report:
                self._reconcile_order_report(diff_report, trades=[], is_external=False)
        elif quantities_match and report.avg_px_open is not None:
            # Quantities match, but verify avg_px_open also matches
            current_avg_px = self._calculate_weighted_avg_px(effective_positions)

            if current_avg_px is not None:
                # Check if avg_px matches within tolerance
                avg_px_diff = abs(current_avg_px - report.avg_px_open)
                relative_diff = avg_px_diff / report.avg_px_open if report.avg_px_open != 0 else 0

                if relative_diff > Decimal("0.0001"):  # 0.01% tolerance
                    self._log.warning(
                        f"Position avg_px mismatch for {report.instrument_id} after reconciliation: "
                        f"internal={current_avg_px}, venue={report.avg_px_open}, "
                        f"diff={avg_px_diff} ({relative_diff * 100:.4f}%). "
                        f"This indicates incomplete reconciliation data from the venue.",
                        LogColor.YELLOW,
                    )
                else:
                    self._log.info(
                        f"Position avg_px verified for {report.instrument_id}: "
                        f"internal={current_avg_px}, venue={report.avg_px_open}",
                        LogColor.BLUE,
                    )

        return True  # Reconciled

    def _publish_startup_position_reconciliation_alert(
        self,
        *,
        report: PositionStatusReport,
        message: str,
        cached_qty: Decimal,
        venue_qty: Decimal,
        position_ids: list[str] | None = None,
        raw_qty: Decimal | None = None,
        effective_qty: Decimal | None = None,
    ) -> None:
        snapshot = self._startup_snapshot_for_instrument(report.account_id, report.instrument_id)
        strategy_ids = [
            snapshot_entry.strategy_id
            for snapshot_entry in snapshot.strategy_snapshots
            if snapshot_entry.strategy_id.value != "EXTERNAL"
        ] or [StrategyId("EXTERNAL")]

        ts_event = int(report.ts_last or report.ts_init or self._clock.timestamp_ns())
        for strategy_id in strategy_ids:
            payload: dict[str, Any] = {
                "strategy_id": strategy_id.value,
                "level": "error",
                "message": message,
                "alert_key": "startup_position_reconciliation",
                "actionable": True,
                "source": "execution",
                "event_type": "StartupPositionReconciliation",
                "venue": report.instrument_id.venue.value,
                "instrument_id": report.instrument_id.value,
                "ts_event": ts_event,
                "ts_ms": ts_event // 1_000_000,
                "cached_qty": str(cached_qty),
                "venue_qty": str(venue_qty),
            }
            if report.account_id is not None:
                payload["account_id"] = report.account_id.value
            if position_ids:
                payload["position_ids"] = position_ids
            if raw_qty is not None:
                payload["raw_qty"] = str(raw_qty)
            if effective_qty is not None:
                payload["effective_qty"] = str(effective_qty)
            self._publish_execution_alert(payload)

    def _reconcile_cross_zero_position(
        self,
        report: PositionStatusReport,
        instrument: Instrument,
        position_signed_decimal_qty: Decimal,
        current_avg_px: Decimal | None,
    ) -> bool:
        self._log.info(
            f"Position crosses through zero for {report.instrument_id}: "
            f"current={position_signed_decimal_qty}, target={report.signed_decimal_qty}. "
            f"Splitting reconciliation into two fills: close existing position, then open new position",
            LogColor.BLUE,
        )

        now = self._clock.timestamp_ns()

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
                close_price = quote.ask_price if close_side == OrderSide.BUY else quote.bid_price

        close_result = False
        if close_price:
            # Fix 2: Check for matching cached order before creating synthetic order
            close_avg_px = close_price.as_decimal()
            matching_close_order = self._find_matching_cached_order(
                instrument_id=report.instrument_id,
                order_side=close_side,
                quantity=close_quantity,
                price=close_price,
                avg_px=close_avg_px,
            )

            if matching_close_order:
                self._log.debug(
                    f"Found matching cached order {matching_close_order.client_order_id} "
                    f"for closing position {report.instrument_id}, reusing instead of creating synthetic order",
                )
                close_report = self._create_order_status_report_from_cached_order(
                    cached_order=matching_close_order,
                    instrument_id=report.instrument_id,
                    account_id=report.account_id,
                    order_side=close_side,
                    quantity=close_quantity,
                    filled_qty=close_quantity,
                    price=close_price,
                    avg_px=close_avg_px,
                    ts_now=now,
                    venue_position_id=report.venue_position_id,
                )
            else:
                close_report = OrderStatusReport(
                    instrument_id=report.instrument_id,
                    account_id=report.account_id,
                    venue_order_id=VenueOrderId(str(UUID4())),
                    venue_position_id=report.venue_position_id,
                    order_side=close_side,
                    order_type=OrderType.LIMIT,
                    time_in_force=TimeInForce.GTC,
                    order_status=OrderStatus.FILLED,
                    price=close_price,
                    quantity=close_quantity,
                    filled_qty=close_quantity,
                    avg_px=close_avg_px,
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
                open_price = quote.ask_price if open_side == OrderSide.BUY else quote.bid_price
            elif close_price:
                # Only allow fallback for CurrencyPair since spot asset positions may lack cost basis
                is_currency_pair = isinstance(instrument, CurrencyPair)

                if is_currency_pair:
                    open_price = close_price
                    self._log.warning(
                        f"Using close price {close_price} as fallback for opening position "
                        f"in cross-zero reconciliation for {report.instrument_id}; "
                        f"venue position report lacks avg_px_open (spot asset position without cost basis)",
                    )
                else:
                    self._log.error(
                        f"Cannot determine open price for {report.instrument_id}: "
                        f"venue position report lacks avg_px_open and no quote tick available; "
                        f"this fallback is only allowed for CurrencyPair (spot asset) positions",
                    )
            else:
                self._log.error(
                    f"Cannot determine open price for {report.instrument_id}: "
                    f"no close price available (existing position lacks avg_px), "
                    f"venue position report lacks avg_px_open, and no quote tick available",
                )

        open_result = False
        if open_price:
            # Fix 2: Check for matching cached order before creating synthetic order
            open_avg_px = open_price.as_decimal()
            matching_open_order = self._find_matching_cached_order(
                instrument_id=report.instrument_id,
                order_side=open_side,
                quantity=open_quantity,
                price=open_price,
                avg_px=open_avg_px,
            )

            if matching_open_order:
                self._log.debug(
                    f"Found matching cached order {matching_open_order.client_order_id} "
                    f"for opening position {report.instrument_id}, reusing instead of creating synthetic order",
                )
                open_report = self._create_order_status_report_from_cached_order(
                    cached_order=matching_open_order,
                    instrument_id=report.instrument_id,
                    account_id=report.account_id,
                    order_side=open_side,
                    quantity=open_quantity,
                    filled_qty=open_quantity,
                    price=open_price,
                    avg_px=open_avg_px,
                    ts_now=now,
                    venue_position_id=report.venue_position_id,
                )
            else:
                open_report = OrderStatusReport(
                    instrument_id=report.instrument_id,
                    account_id=report.account_id,
                    venue_order_id=VenueOrderId(str(UUID4())),
                    venue_position_id=report.venue_position_id,
                    order_side=open_side,
                    order_type=OrderType.LIMIT,
                    time_in_force=TimeInForce.GTC,
                    order_status=OrderStatus.FILLED,
                    price=open_price,
                    quantity=open_quantity,
                    filled_qty=open_quantity,
                    avg_px=open_avg_px,
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

    def _create_position_reconciliation_report(
        self,
        report: PositionStatusReport,
        instrument: Instrument,
        position_signed_decimal_qty: Decimal,
        diff_quantity: Quantity,
        current_avg_px: Decimal | None,
    ) -> OrderStatusReport | None:
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

        now = self._clock.timestamp_ns()

        if reconciliation_price:
            # Generate a LIMIT order with the calculated reconciliation price
            avg_px = reconciliation_price.as_decimal()

            # Only reuse cached orders for netting mode - hedge mode positions are tracked
            # separately and reusing orders could match the wrong position
            matching_diff_order = None
            if report.venue_position_id is None:
                matching_diff_order = self._find_matching_cached_order(
                    instrument_id=report.instrument_id,
                    order_side=order_side,
                    quantity=diff_quantity,
                    price=reconciliation_price,
                    avg_px=avg_px,
                )

            if matching_diff_order:
                self._log.debug(
                    f"Found matching cached order {matching_diff_order.client_order_id} "
                    f"for position reconciliation {report.instrument_id}, reusing instead of creating synthetic order",
                )
                return self._create_order_status_report_from_cached_order(
                    cached_order=matching_diff_order,
                    instrument_id=report.instrument_id,
                    account_id=report.account_id,
                    order_side=order_side,
                    quantity=diff_quantity,
                    filled_qty=diff_quantity,
                    price=reconciliation_price,
                    avg_px=avg_px,
                    ts_now=now,
                    venue_position_id=report.venue_position_id,
                )
            else:
                return OrderStatusReport(
                    instrument_id=report.instrument_id,
                    account_id=report.account_id,
                    venue_order_id=VenueOrderId(str(UUID4())),
                    venue_position_id=report.venue_position_id,
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

            # Only reuse cached orders for netting mode
            matching_diff_order = None
            if report.venue_position_id is None:
                matching_diff_order = self._find_matching_cached_order(
                    instrument_id=report.instrument_id,
                    order_side=order_side,
                    quantity=diff_quantity,
                    price=None,
                    avg_px=None,
                )

            if matching_diff_order:
                self._log.debug(
                    f"Found matching cached order {matching_diff_order.client_order_id} "
                    f"for position reconciliation {report.instrument_id}, reusing instead of creating synthetic order",
                )
                return self._create_order_status_report_from_cached_order(
                    cached_order=matching_diff_order,
                    instrument_id=report.instrument_id,
                    account_id=report.account_id,
                    order_side=order_side,
                    quantity=diff_quantity,
                    filled_qty=diff_quantity,
                    price=None,
                    avg_px=avg_px,
                    ts_now=now,
                    venue_position_id=report.venue_position_id,
                )
            else:
                return OrderStatusReport(
                    instrument_id=report.instrument_id,
                    account_id=report.account_id,
                    venue_order_id=VenueOrderId(str(UUID4())),
                    venue_position_id=report.venue_position_id,
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

    def _reconcile_order_report(
        self,
        report: OrderStatusReport,
        trades: list[FillReport],
        is_external: bool = True,
    ) -> bool:
        if self._is_shutting_down:
            return True  # Skip reconciliation during shutdown

        client_order_id = self._resolve_client_order_id(report)

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

            # Explicitly index venue_order_id for external orders to ensure they can be found
            # by venue_order_id in subsequent reconciliation passes
            if order.venue_order_id is not None:
                self._ensure_venue_order_id_indexed(
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                )

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

        # Handle order status transitions
        status_result = self._handle_order_status_transitions(order, report, trades, instrument)
        if status_result is not None:
            return status_result

        # Reconcile all trades
        for trade in trades:
            self._reconcile_fill_report(order, trade, instrument)

        if report.avg_px is None:
            self._log.warning("report.avg_px was `None` when a value was expected")

        # Handle fill quantity mismatches
        return self._handle_fill_quantity_mismatch(order, report, instrument, client_order_id)

    def _resolve_client_order_id(self, report: OrderStatusReport) -> ClientOrderId:
        client_order_id: ClientOrderId | None = report.client_order_id
        if client_order_id is None:
            client_order_id = self._cache.client_order_id(report.venue_order_id)
            if client_order_id is None and report.venue_order_id is not None:
                # Check if an external order with this venue_order_id already exists
                # by searching cached orders (handles cases where index might not be built yet)
                cached_order = self._find_order_by_venue_order_id(
                    venue_order_id=report.venue_order_id,
                    instrument_id=report.instrument_id,
                    order_side=report.order_side,
                )
                if cached_order is not None:
                    client_order_id = cached_order.client_order_id
                    self._log.debug(
                        f"Found existing external order {client_order_id} by venue_order_id "
                        f"{report.venue_order_id}, reusing",
                    )
                    # Ensure mapping is indexed
                    self._ensure_venue_order_id_indexed(
                        client_order_id=client_order_id,
                        venue_order_id=report.venue_order_id,
                    )

            if client_order_id is None:
                # Generate external client order ID
                client_order_id = ClientOrderId(UUID4().value)

            # Assign to report
            report.client_order_id = client_order_id

        return client_order_id

    def _ensure_venue_order_id_indexed(
        self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        log_context: str = "",
    ) -> None:
        # Index venue_order_id in cache for lookups
        try:
            self._cache.add_venue_order_id(
                client_order_id,
                venue_order_id,
                overwrite=False,
            )
        except ValueError:
            # Mapping already exists or conflicts - this is expected if order was
            # previously indexed or if there's a conflict (which should be rare)
            self._log.debug(
                f"Venue order ID {venue_order_id} already indexed for "
                f"{client_order_id}{' ' + log_context if log_context else ''}, skipping",
            )

    def _handle_fill_quantity_mismatch(
        self,
        order: Order,
        report: OrderStatusReport,
        instrument: Instrument,
        client_order_id: ClientOrderId,
    ) -> bool:
        if report.filled_qty < order.filled_qty:
            # Gather diagnostic information
            fill_history = [
                (event.trade_id, event.last_qty, event.ts_event)
                for event in order.events
                if isinstance(event, OrderFilled)
            ]

            self._log.error(
                f"report.filled_qty {report.filled_qty} < order.filled_qty {order.filled_qty}, "
                f"this could potentially be caused by duplicate fills or corrupted cached state; "
                f"order_id={order.client_order_id}, venue_order_id={order.venue_order_id}, "
                f"total_fills_applied={len(fill_history)}, "
                f"fill_trade_ids={order.trade_ids}, "
                f"inferred_fill={'yes' if client_order_id in self._inferred_fill_ts else 'no'}, "
                f"order_status={order.status}, report_status={report.order_status}",
            )

            # Log each fill for forensics
            for trade_id, qty, ts in fill_history:
                self._log.error(f"  Fill: {trade_id}, qty={qty}, ts={ts}")

            return False  # Failed

        if report.filled_qty > order.filled_qty:
            # Check if order is already closed to avoid duplicate inferred fills
            if order.is_closed:
                # Use the higher precision for tolerance check
                precision = max(report.filled_qty.precision, order.filled_qty.precision)
                if is_within_single_unit_tolerance(
                    report.filled_qty.as_decimal(),
                    order.filled_qty.as_decimal(),
                    precision,
                ):
                    return True

                self._log.debug(  # TODO: Reduce level to debug after initial development phase
                    f"{order.instrument_id} {order.client_order_id!r} already {order.status_string()} but "
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

    def _handle_order_status_transitions(
        self,
        order: Order,
        report: OrderStatusReport,
        trades: list[FillReport],
        instrument: Instrument,
    ) -> bool | None:
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

                # Reconcile all trades before expired event (same as canceled)
                for trade in trades:
                    self._reconcile_fill_report(order, trade, instrument)

                self._generate_order_expired(order, report)

            return True  # Reconciled

        return None  # Continue with fill reconciliation

    def _should_update(self, order: Order, report: OrderStatusReport) -> bool:
        if report.quantity != order.quantity and report.quantity >= order.filled_qty:
            return True  # Valid quantity update

        match order.order_type:
            case OrderType.LIMIT:
                return report.price != order.price
            case OrderType.STOP_MARKET | OrderType.TRAILING_STOP_MARKET:
                return report.trigger_price != order.trigger_price
            case OrderType.STOP_LIMIT | OrderType.TRAILING_STOP_LIMIT:
                return report.trigger_price != order.trigger_price or report.price != order.price
            case _:
                return False

    def _reconcile_fill_report(
        self,
        order: Order,
        report: FillReport,
        instrument: Instrument,
    ) -> bool:
        # Check if this fill should be skipped (predates inferred fill or is duplicate)
        skip_result = self._check_and_skip_duplicate_fill(order, report)
        if skip_result is not None:
            return skip_result

        # Check if fill would cause overfill
        potential_filled_qty = order.filled_qty + report.last_qty
        if potential_filled_qty > order.quantity:
            if not self.allow_overfills:
                self._log.warning(
                    f"Rejecting fill that would cause overfill for {order.client_order_id!r}: "
                    f"order.quantity={order.quantity}, order.filled_qty={order.filled_qty}, "
                    f"fill.last_qty={report.last_qty}, would result in filled_qty={potential_filled_qty}",
                )
                return False  # Reject fill to prevent overfill
            # allow_overfills=True: log warning but allow the fill through
            self._log.warning(
                f"Allowing overfill during reconciliation for {order.client_order_id!r}: "
                f"order.quantity={order.quantity}, order.filled_qty={order.filled_qty}, "
                f"fill.last_qty={report.last_qty}, will result in filled_qty={potential_filled_qty}",
            )

        # Verify total fills consistency BEFORE applying
        current_total = sum(
            event.last_qty for event in order.events if isinstance(event, OrderFilled)
        )
        if current_total != order.filled_qty:
            self._log.error(
                f"INCONSISTENCY DETECTED before applying fill: "
                f"sum(fills)={current_total} != order.filled_qty={order.filled_qty} "
                f"for {order.client_order_id}",
            )

        # Final check: ensure trade_id doesn't already exist before generating fill
        # This prevents KeyError from being raised in _apply_event_to_order
        existing_fill = get_existing_fill_for_trade_id(order, report.trade_id)
        if report.trade_id in order.trade_ids or existing_fill is not None:
            self._log.debug(
                f"Fill with trade_id {report.trade_id} already exists for order {order.client_order_id}, skipping duplicate",
            )
            return True  # Fill already exists, treat as successful

        # Track fill application in audit trail BEFORE generating the fill
        # This ensures cleanup on close remains effective if this fill closes the order
        if order.client_order_id not in self._fill_application_audit:
            self._fill_application_audit[order.client_order_id] = []

        audit_entry = (report.trade_id, "reconciliation", self._clock.timestamp_ns())
        self._fill_application_audit[order.client_order_id].append(audit_entry)

        try:
            self._generate_order_filled(order, report, instrument)
        except InvalidStateTrigger as e:
            self._rollback_fill_audit_entry(order.client_order_id, audit_entry)
            self._log.error(str(e))
            return False
        except ValueError as e:
            self._rollback_fill_audit_entry(order.client_order_id, audit_entry)
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

    def _check_and_skip_duplicate_fill(
        self,
        order: Order,
        report: FillReport,
    ) -> bool | None:
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

        # Check for duplicate fill by trade_id - check both trade_ids collection and events
        # This handles cases where order is loaded from cache and trade_ids might not be fully populated
        existing_fill = get_existing_fill_for_trade_id(order, report.trade_id)
        if report.trade_id in order.trade_ids or existing_fill is not None:
            # Fill already applied; check if data is consistent.
            # An existing fill may be sourced from the cache on start,
            # or may exist in-memory when a reconciliation is triggered.

            # Log detailed info about when it was first applied
            if order.client_order_id in self._fill_application_audit:
                audit = self._fill_application_audit[order.client_order_id]
                previous = [a for a in audit if a[0] == report.trade_id]
                if previous:
                    self._log.debug(
                        f"Duplicate fill detected; {report.trade_id} was already applied "
                        f"at ts={previous[0][2]}, source={previous[0][1]}",
                    )

            if existing_fill and not self._fill_reports_equal(existing_fill, report):
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

            # If trade_id is in order.trade_ids or we found an existing fill, skip this fill
            # This prevents duplicate fills from being applied
            return True  # Fill already applied, continue with existing data

        return None  # Not a duplicate, proceed with fill

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

    # -- ORDER AND EVENTS GENERATION ---------------------------------------------------------------

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

        # Only venue-originated external orders should be claimed by strategies.
        # Synthetic reconciliation orders must remain EXTERNAL so they don't
        # pollute strategy-owned state when cache is reloaded on restart.
        strategy_id = self.get_external_order_claim(report.instrument_id) if is_external else None

        if strategy_id is None:
            # All unclaimed reconciliation uses EXTERNAL strategy ID
            # Tags distinguish the source for filtering purposes
            strategy_id = StrategyId("EXTERNAL")
            if is_external:
                # Actual external order found on venue
                tags = ["VENUE"]
            else:
                # Internal position diff alignment (synthetic fill)
                tags = ["RECONCILIATION"]
        else:
            # External order claimed by a strategy via external_order_claims config
            # This order will be managed by the claiming strategy
            tags = None
            self._log.info(
                f"External order {report.client_order_id} for {report.instrument_id} "
                f"claimed by strategy {strategy_id}",
                LogColor.BLUE,
            )

        # Filter unclaimed external orders (but not reconciliation fills)
        if self.filter_unclaimed_external_orders and tags and "VENUE" in tags:
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

    # -- INTERNAL ----------------------------------------------------------------------------------

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
        # Handle an order event with activity tracking, recording fills in cache and
        # cleaning up tracking data for closed orders.
        self._record_local_activity(event)
        cancel_rejected_prior_status: OrderStatus | None = None

        if isinstance(event, OrderCancelRejected) and event.client_order_id is not None:
            cached_order = self._cache.order(event.client_order_id)
            if cached_order is not None:
                cancel_rejected_prior_status = cached_order.status

        if isinstance(event, OrderFilled):
            self._recent_fills_cache[event.trade_id] = self._clock.timestamp_ns()
            self._position_local_activity_ns[event.instrument_id] = event.ts_event

            # Track inferred fill timestamps to prevent duplicate historical fills
            if event.reconciliation:
                client_order_id = event.client_order_id
                if client_order_id not in self._inferred_fill_ts:
                    self._inferred_fill_ts[client_order_id] = event.ts_event

        self._handle_event(event)
        self._resolve_cancel_state_mismatch(
            event,
            prior_status=cancel_rejected_prior_status,
        )
        self._publish_execution_alert_if_relevant(event)

        if event.client_order_id is None:
            return

        order = self._cache.order(event.client_order_id)
        if order and order.is_closed:
            self._clear_recon_tracking(order.client_order_id)
            self._order_local_activity_ns.pop(order.client_order_id, None)
            self._inferred_fill_ts.pop(order.client_order_id, None)
            self._fill_application_audit.pop(order.client_order_id, None)

    def _resolve_cached_order_missing_at_venue(
        self,
        order: Order,
        *,
        ts_now: int,
        reason: str,
        prior_status: OrderStatus | None = None,
    ) -> None:
        if not order.is_open:
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
            else:
                self._log.debug(
                    f"Skipping missing-order resolution for {order.client_order_id!r} - "
                    f"current status {order.status_string()}",
                )
            self._clear_recon_tracking(order.client_order_id)
            self._order_local_activity_ns.pop(order.client_order_id, None)
            return

        resolution_status = prior_status or order.status

        if resolution_status in (OrderStatus.ACCEPTED, OrderStatus.SUBMITTED):
            self._log.warning(
                f"Reconciling {order.client_order_id!r}: {resolution_status.name} "
                f"order not found at venue, marking as REJECTED",
                LogColor.YELLOW,
            )
            rejected = create_order_rejected_event(
                order=order,
                ts_now=ts_now,
                reason=reason,
            )
            self._handle_event_with_tracking(rejected)
            self._clear_recon_tracking(order.client_order_id)
            self._order_local_activity_ns.pop(order.client_order_id, None)
            return

        if resolution_status in (
            OrderStatus.PARTIALLY_FILLED,
            OrderStatus.PENDING_CANCEL,
            OrderStatus.PENDING_UPDATE,
        ):
            self._log.warning(
                f"Reconciling {order.client_order_id!r}: {resolution_status.name} "
                f"order not found at venue, marking as CANCELED",
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

        self._log.warning(
            f"Unexpected order status {order.status_string()} "
            f"for order not found at venue: {order.client_order_id!r}",
        )
        self._clear_recon_tracking(order.client_order_id)
        self._order_local_activity_ns.pop(order.client_order_id, None)

    def _resolve_cancel_state_mismatch(
        self,
        event: OrderEvent,
        *,
        prior_status: OrderStatus | None = None,
    ) -> None:
        if not isinstance(event, OrderCancelRejected):
            return

        if not self._is_cancel_state_mismatch_reason(self._normalize_alert_reason(event.reason)):
            return

        client_order_id = event.client_order_id
        if client_order_id is None:
            return

        order = self._cache.order(client_order_id)
        if order is None or order.is_closed:
            return

        self._resolve_cached_order_missing_at_venue(
            order,
            ts_now=int(event.ts_event or self._clock.timestamp_ns()),
            reason="ORDER_CANCEL_STATE_MISMATCH",
            prior_status=prior_status,
        )

    @staticmethod
    def _normalize_alert_reason(reason: str | None) -> str:
        return " ".join((reason or "").strip().lower().split())

    def _owned_strategy_id_for_event(self, event: OrderEvent) -> StrategyId | None:
        strategy_id = None
        if event.client_order_id is not None:
            strategy_id = self._cache.strategy_id_for_order(event.client_order_id)

        if strategy_id is None:
            strategy_id = getattr(event, "strategy_id", None)

        if strategy_id is None or strategy_id.value == "EXTERNAL":
            return None

        return strategy_id

    @classmethod
    def _is_financial_reject_reason(cls, reason: str) -> bool:
        return any(
            text in reason
            for text in (
                "insufficient account balance",
                "insufficient balance",
                "insufficient margin",
                "insufficient funds",
                "insufficient equity",
            )
        )

    @classmethod
    def _is_terminal_reject_reason(cls, reason: str) -> bool:
        return reason.startswith("unsupported_account_mode")

    @classmethod
    def _is_cancel_state_mismatch_reason(cls, reason: str) -> bool:
        return any(
            text in reason
            for text in (
                "unknown order sent",
                "s_code=51400",
                "filled, canceled or does not exist",
                "filled, cancelled or does not exist",
                "already canceled",
                "already cancelled",
                "already canceled or matched",
                "state mismatch",
                "does not exist",
            )
        )

    def _publish_execution_alert(self, payload: dict[str, Any]) -> None:
        payload_json = json.dumps(payload, sort_keys=True, separators=(",", ":"))
        self._msgbus.publish(
            topic=TOPIC_EXECUTION_ALERT,
            msg=FluxBusPayload(
                topic=TOPIC_EXECUTION_ALERT,
                payload=payload_json,
                ts_event=int(payload.get("ts_event", 0)),
                ts_init=int(payload.get("ts_event", 0)),
            ),
        )

    def _build_execution_alert_payload(
        self,
        *,
        event: OrderEvent,
        strategy_id: StrategyId,
        alert_key: str,
        message: str,
        reason: str,
    ) -> dict[str, Any]:
        ts_event = int(event.ts_event or self._clock.timestamp_ns())
        venue = event.instrument_id.venue.value if event.instrument_id is not None else ""
        payload: dict[str, Any] = {
            "strategy_id": strategy_id.value,
            "level": "error",
            "message": message,
            "alert_key": alert_key,
            "actionable": True,
            "source": "execution",
            "event_type": type(event).__name__,
            "venue": venue,
            "instrument_id": event.instrument_id.value if event.instrument_id is not None else "",
            "client_order_id": event.client_order_id.value if event.client_order_id else "",
            "ts_event": ts_event,
            "ts_ms": ts_event // 1_000_000,
        }
        if reason:
            payload["reason"] = reason
        venue_order_id = getattr(event, "venue_order_id", None)
        if venue_order_id is not None:
            payload["venue_order_id"] = venue_order_id.value
        return payload

    def _publish_execution_alert_with_cooldown(
        self,
        *,
        event: OrderEvent,
        strategy_id: StrategyId,
        reason: str,
        alert_key: str,
        message: str,
        cooldown_ns: int,
    ) -> None:
        normalized_reason = self._normalize_alert_reason(reason) or "unknown"
        now_ns = int(event.ts_event or self._clock.timestamp_ns())
        cooldown_key = (strategy_id.value, alert_key, normalized_reason)
        last_sent_ns = self._execution_alert_last_sent_ns.get(cooldown_key, 0)
        if last_sent_ns and now_ns - last_sent_ns < cooldown_ns:
            return

        self._execution_alert_last_sent_ns[cooldown_key] = now_ns
        self._publish_execution_alert(
            self._build_execution_alert_payload(
                event=event,
                strategy_id=strategy_id,
                alert_key=alert_key,
                message=message,
                reason=reason,
            ),
        )

    def _publish_burst_execution_alert_if_needed(
        self,
        *,
        event: OrderEvent,
        strategy_id: StrategyId,
        reason: str,
        alert_key: str,
        message: str,
    ) -> None:
        normalized_reason = self._normalize_alert_reason(reason) or "unknown"
        now_ns = int(event.ts_event or self._clock.timestamp_ns())
        burst_key = (strategy_id.value, event.instrument_id.venue.value, normalized_reason)
        window = [
            ts
            for ts in self._execution_alert_windows.get(burst_key, [])
            if now_ns - ts <= self._EXECUTION_ALERT_BURST_WINDOW_NS
        ]
        window.append(now_ns)
        self._execution_alert_windows[burst_key] = window

        if len(window) < self._EXECUTION_ALERT_BURST_THRESHOLD:
            return

        cooldown_key = (strategy_id.value, alert_key, normalized_reason)
        last_sent_ns = self._execution_alert_last_sent_ns.get(cooldown_key, 0)
        if now_ns - last_sent_ns < self._EXECUTION_ALERT_BURST_COOLDOWN_NS:
            return

        self._execution_alert_last_sent_ns[cooldown_key] = now_ns
        self._publish_execution_alert(
            self._build_execution_alert_payload(
                event=event,
                strategy_id=strategy_id,
                alert_key=alert_key,
                message=message,
                reason=reason,
            ),
        )

    def _publish_execution_alert_if_relevant(self, event: OrderEvent) -> None:
        if event.reconciliation:
            return

        strategy_id = self._owned_strategy_id_for_event(event)
        if strategy_id is None:
            return

        reason = self._normalize_alert_reason(getattr(event, "reason", None))

        if isinstance(event, OrderDenied):
            self._publish_execution_alert_with_cooldown(
                event=event,
                strategy_id=strategy_id,
                alert_key="order_denied",
                message=f"Order denied before exchange submission on {event.instrument_id.venue}: {event.reason}",
                reason=event.reason,
                cooldown_ns=self._EXECUTION_ALERT_BURST_COOLDOWN_NS,
            )
            return

        if isinstance(event, OrderRejected):
            if event.due_post_only:
                return
            if self._is_financial_reject_reason(reason):
                self._publish_execution_alert_with_cooldown(
                    event=event,
                    strategy_id=strategy_id,
                    alert_key="exchange_order_rejected_insufficient_margin",
                    message=f"Exchange rejected order on {event.instrument_id.venue}: {event.reason}",
                    reason=event.reason,
                    cooldown_ns=self._EXECUTION_ALERT_BURST_COOLDOWN_NS,
                )
            elif self._is_terminal_reject_reason(reason):
                self._publish_execution_alert_with_cooldown(
                    event=event,
                    strategy_id=strategy_id,
                    alert_key="exchange_order_rejected",
                    message=f"Exchange rejected order on {event.instrument_id.venue}: {event.reason}",
                    reason=event.reason,
                    cooldown_ns=self._EXECUTION_ALERT_BURST_COOLDOWN_NS,
                )
            else:
                self._publish_execution_alert(
                    self._build_execution_alert_payload(
                        event=event,
                        strategy_id=strategy_id,
                        alert_key="exchange_order_rejected",
                        message=f"Exchange rejected order on {event.instrument_id.venue}: {event.reason}",
                        reason=event.reason,
                    ),
                )
            return

        if isinstance(event, OrderCancelRejected):
            if self._is_cancel_state_mismatch_reason(reason):
                return

            self._publish_execution_alert(
                self._build_execution_alert_payload(
                    event=event,
                    strategy_id=strategy_id,
                    alert_key="exchange_order_cancel_rejected",
                    message=f"Exchange cancel rejected on {event.instrument_id.venue}: {event.reason}",
                    reason=event.reason,
                ),
            )
            return

        if isinstance(event, OrderModifyRejected):
            if self._is_financial_reject_reason(reason):
                self._publish_execution_alert_with_cooldown(
                    event=event,
                    strategy_id=strategy_id,
                    alert_key="exchange_order_modify_rejected_insufficient_margin",
                    message=f"Exchange modify rejected on {event.instrument_id.venue}: {event.reason}",
                    reason=event.reason,
                    cooldown_ns=self._EXECUTION_ALERT_BURST_COOLDOWN_NS,
                )
            else:
                self._publish_execution_alert(
                    self._build_execution_alert_payload(
                        event=event,
                        strategy_id=strategy_id,
                        alert_key="exchange_order_modify_rejected",
                        message=f"Exchange modify rejected on {event.instrument_id.venue}: {event.reason}",
                        reason=event.reason,
                    ),
                )

    def _record_local_activity(self, event: OrderEvent | None) -> None:
        if event is None:
            return

        client_order_id = event.client_order_id
        if client_order_id is None:
            return

        # Use receipt time (current clock time) instead of venue time (ts_event)
        # to accurately track when we last processed activity for this order.
        # This avoids race conditions where network/queue latency makes events
        # appear "old" even though they just arrived.
        self._order_local_activity_ns[client_order_id] = self._clock.timestamp_ns()

    def _find_matching_cached_order(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price | None,
        avg_px: Decimal | None,
    ) -> Order | None:
        # Search cache for existing order matching reconciliation parameters
        cached_orders = self._cache.orders(
            instrument_id=instrument_id,
            venue=None,
            side=order_side,
        )

        for cached_order in cached_orders:
            # Check if order is filled and matches the parameters
            if cached_order.status != OrderStatus.FILLED:
                continue

            # Match quantity
            if cached_order.filled_qty != quantity:
                continue

            # Match price if provided (market orders don't have price)
            if price is not None and cached_order.has_price and cached_order.price != price:
                continue

            # Match avg_px if provided
            if avg_px is not None and cached_order.avg_px is not None:
                cached_avg_px = Decimal(str(cached_order.avg_px))
                if cached_avg_px != avg_px:
                    continue

            # Found a match
            return cached_order

        return None

    def _find_order_by_venue_order_id(
        self,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        order_side: OrderSide | None = None,
    ) -> Order | None:
        # Fallback search when venue_order_id index not built
        cached_orders = self._cache.orders(
            venue=instrument_id.venue,
            instrument_id=instrument_id,
            side=order_side,
        )

        for cached_order in cached_orders:
            if cached_order.venue_order_id == venue_order_id:
                return cached_order

        return None
