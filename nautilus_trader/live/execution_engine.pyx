# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime
from cpython.datetime cimport timedelta

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.message cimport Message
from nautilus_trader.core.message cimport MessageCategory
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.execution.messages cimport ExecutionMassStatus
from nautilus_trader.execution.messages cimport OrderStatusReport
from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.commands.trading cimport TradingCommand
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.msgbus.message_bus cimport MessageBus


cdef class LiveExecutionEngine(ExecutionEngine):
    """
    Provides a high-performance asynchronous live execution engine.
    """
    _sentinel = None

    def __init__(
        self,
        loop not None: asyncio.AbstractEventLoop,
        TraderId trader_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the ``LiveExecutionEngine`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the engine.
        trader_id : TraderId
            The trader ID for the engine.
        msgbus : MessageBus
            The message bus for the engine.
        cache : Cache
            The cache for the engine.
        clock : Clock
            The clock for the engine.
        logger : Logger
            The logger for the engine.
        config : dict[str, object], optional
            The configuration options.

        """
        if config is None:
            config = {}
        super().__init__(
            trader_id=trader_id,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop = loop
        self._queue = Queue(maxsize=config.get("qsize", 10000))

        self._run_queue_task = None
        self.is_running = False

    def get_event_loop(self) -> asyncio.AbstractEventLoop:
        """
        Return the internal event loop for the engine.

        Returns
        -------
        asyncio.AbstractEventLoop

        """
        return self._loop

    def get_run_queue_task(self) -> asyncio.Task:
        """
        Return the internal run queue task for the engine.

        Returns
        -------
        asyncio.Task

        """
        return self._run_queue_task

    cpdef int qsize(self) except *:
        """
        Return the number of messages buffered on the internal queue.

        Returns
        -------
        int

        """
        return self._queue.qsize()

    async def reconcile_state(self, double timeout_secs) -> bool:
        """
        Reconcile the execution engines state with all execution clients.

        The execution engine will collect all cached active orders and send
        those to the relevant execution client(s) for a comparison with the
        exchange(s) order states.

        If a cached order does not match the exchanges order status then
        the missing events will be generated. If there is not enough information
        to reconcile a state then errors will be logged.

        Parameters
        ----------
        timeout_secs : double
            The seconds to allow for reconciliation before timing out.

        Returns
        -------
        bool
            True if states reconcile within timeout, else False.

        Raises
        ------
        ValueError
            If timeout_secs is not positive (> 0).

        """
        Condition.positive(timeout_secs, "timeout_secs")
        cdef dict active_orders = {
            order.client_order_id: order for order in self.cache.orders() if not order.is_completed_c()
        }  # type: dict[ClientOrderId, Order]

        if not active_orders:
            return True  # Execution states reconciled

        cdef int count = len(active_orders)
        self._log.info(
            f"Reconciling state: {count} active order{'s' if count > 1 else ''}...",
            color=LogColor.BLUE,
        )

        # Initialize order state map
        cdef dict client_orders = {
            name: [] for name in self._clients.keys()
        }   # type: dict[ClientId, list[Order]]

        # Build order state map
        cdef Order order
        cdef LiveExecutionClient client
        for order in active_orders.values():
            client = self._routing_map.get(order.instrument_id.venue)
            if client is None:
                self._log.error(
                    f"Cannot reconcile state: "
                    f"No client found for {order.instrument_id.venue} for active {order}."
                )
                continue
            client_orders[client.id].append(order)

        cdef dict client_mass_status = {}  # type: dict[ClientId, ExecutionMassStatus]

        # Generate state report for each client
        for name, client in self._clients.items():
            client_mass_status[name] = await client.generate_mass_status(client_orders[name])

        # Reconcile order states
        cdef ExecutionMassStatus mass_status
        cdef OrderStatusReport order_state_report
        for name, mass_status in client_mass_status.items():
            order_reports = mass_status.order_reports()
            if not order_reports:
                continue
            for order_state_report in order_reports.values():
                order = active_orders.get(order_state_report.client_order_id)
                if order is None:
                    self._log.error(
                        f"Cannot reconcile state: "
                        f"No order found for {repr(order_state_report.client_order_id)}."
                    )
                    continue
                exec_reports = mass_status.exec_reports().get(order.venue_order_id, [])
                await self._clients[name].reconcile_state(order_state_report, order, exec_reports)

        # Wait for state resolution until timeout...
        cdef datetime timeout = self._clock.utc_now() + timedelta(seconds=timeout_secs)
        cdef OrderStatusReport report
        while True:
            reconciled = True
            for order in active_orders.values():
                client = self._routing_map.get(order.instrument_id.venue)
                if client is None:
                    self._log.error(
                        f"Cannot reconcile state: "
                        f"No client found for {order.instrument_id.venue}."
                    )
                    return False  # Will never reconcile
                mass_status = client_mass_status.get(client.id)
                if mass_status is None:
                    return False  # Will never reconcile
                report = mass_status.order_reports().get(order.venue_order_id)
                if report is None:
                    return False  # Will never reconcile
                if order.state_c() != report.order_state:
                    reconciled = False  # Incorrect state on this loop
                if report.order_state in (OrderState.FILLED, OrderState.PARTIALLY_FILLED):
                    if order.filled_qty != report.filled_qty:
                        reconciled = False  # Incorrect filled quantity on this loop
            if reconciled:
                break
            if self._clock.utc_now() >= timeout:
                return False
            await asyncio.sleep(0)  # Sleep for one event loop cycle

        return True  # Execution states reconciled

    cpdef void kill(self) except *:
        """
        Kill the engine by abruptly cancelling the queue task and calling stop.
        """
        self._log.warning("Killing engine...")
        if self._run_queue_task:
            self._log.debug("Cancelling run_queue_task...")
            self._run_queue_task.cancel()
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
            self._queue.put_nowait(command)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_queue.put` as queue full "
                f"at {self._queue.qsize()} items.",
            )
            self._loop.create_task(self._queue.put(command))  # Blocking until qsize reduces

    cpdef void process(self, Event event) except *:
        """
        Process the given event.

        If the internal queue is already full then will log a warning and block
        until queue size reduces.

        Parameters
        ----------
        event : Event
            The event to process.

        Warnings
        --------
        This method should only be called from the same thread the event loop is
        running on.

        """
        Condition.not_none(event, "event")

        try:
            self._queue.put_nowait(event)
        except asyncio.QueueFull:
            self._log.warning(
                f"Blocking on `_queue.put` as queue full "
                f"at {self._queue.qsize()} items.",
            )
            self._loop.create_task(self._queue.put(event))  # Blocking until qsize reduces

    cpdef void _on_start(self) except *:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running.")

        self.is_running = True  # Queue will continue to process
        self._run_queue_task = self._loop.create_task(self._run())

        self._log.debug(f"Scheduled {self._run_queue_task}")

    cpdef void _on_stop(self) except *:
        if self.is_running:
            self.is_running = False
            self._enqueue_sentinel()

    async def _run(self):
        self._log.debug(
            f"Message queue processing starting (qsize={self.qsize()})...",
        )
        cdef Message message
        try:
            while self.is_running:
                message = await self._queue.get()
                if message is None:  # Sentinel message (fast C-level check)
                    continue         # Returns to the top to check `self.is_running`
                if message.category == MessageCategory.EVENT:
                    self._handle_event(message)
                elif message.category == MessageCategory.COMMAND:
                    self._execute_command(message)
                else:
                    self._log.error(f"Cannot handle message: unrecognized {message}.")
        except asyncio.CancelledError:
            if not self._queue.empty():
                self._log.warning(
                    f"Running canceled with {self.qsize()} message(s) on queue.",
                )
            else:
                self._log.debug(
                    f"Message queue processing stopped (qsize={self.qsize()}).",
                )

    cdef void _enqueue_sentinel(self) except *:
        self._queue.put_nowait(self._sentinel)
        self._log.debug(f"Sentinel message placed on message queue.")
