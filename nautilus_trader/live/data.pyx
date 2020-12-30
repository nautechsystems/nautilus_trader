# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from asyncio import AbstractEventLoop
from asyncio import CancelledError
import asyncio

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.messages cimport DataRequest
from nautilus_trader.common.messages cimport DataResponse
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Message
from nautilus_trader.core.message cimport MessageType
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.model.commands cimport VenueCommand
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.trading.portfolio cimport Portfolio


cdef class LiveDataEngine(DataEngine):
    """
    Provides a high-performance asynchronous live data engine.
    """

    def __init__(
        self,
        loop not None: AbstractEventLoop,
        Portfolio portfolio not None,
        LiveClock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the `LiveDataEngine` class.

        Parameters
        ----------
        loop : AbstractEventLoop
            The event loop for the engine.
        portfolio : int
            The portfolio to register.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.
        config : dict, option
            The configuration options.

        """
        super().__init__(
            portfolio,
            clock,
            logger,
            config,
        )

        self._loop = loop
        self._data_queue = asyncio.Queue()
        self._message_queue = asyncio.Queue()
        self.is_running = False

    cpdef object get_event_loop(self):
        """
        Return the internal event loop for the engine.

        Returns
        -------
        asyncio.AbstractEventLoop

        """
        return self._loop

    cpdef object get_run_queue_task(self):
        """
        Return the internal run queue task for the engine.

        Returns
        -------
        asyncio.Task

        """
        return self._run_queues_task

    cpdef int data_qsize(self) except *:
        """
        Return the number of objects buffered on the internal data queue.

        Returns
        -------
        int

        """
        return self._data_queue.qsize()

    cpdef int message_qsize(self) except *:
        """
        Return the number of objects buffered on the internal message queue.

        Returns
        -------
        int

        """
        return self._message_queue.qsize()

    cpdef void execute(self, VenueCommand command) except *:
        """
        Execute the given command.

        Parameters
        ----------
        command : VenueCommand
            The command to execute.

        """
        Condition.not_none(command, "command")
        # Do not allow None through (None is a sentinel value which stops the queue)

        self._loop.call_soon_threadsafe(self._message_queue.put_nowait, command)

    cpdef void process(self, data) except *:
        """
        Process the given data.

        Parameters
        ----------
        data : object
            The data to process.

        """
        Condition.not_none(data, "data")
        # Do not allow None through (None is a sentinel value which stops the queue)

        self._loop.call_soon_threadsafe(self._data_queue.put_nowait, data)

    cpdef void send(self, DataRequest request) except *:
        """
        Handle the given request.

        Parameters
        ----------
        request : DataRequest
            The request to handle.

        """
        Condition.not_none(request, "request")
        # Do not allow None through (None is a sentinel value which stops the queue)

        self._loop.call_soon_threadsafe(self._message_queue.put_nowait, request)

    cpdef void receive(self, DataResponse response) except *:
        """
        Handle the given response.

        Parameters
        ----------
        response : DataResponse
            The response to handle.

        """
        Condition.not_none(response, "response")
        # Do not allow None through (None is a sentinel value which stops the queue)

        self._loop.call_soon_threadsafe(self._message_queue.put_nowait, response)

    cpdef void _on_start(self) except *:
        if not self._loop.is_running():
            self._log.warning("Started when loop is not running.")

        # Ensure this is set True so that below queues continue to process
        self.is_running = True

        # Run queues
        self._run_queues_task = asyncio.gather(
            self._loop.create_task(self._run_data_queue()),
            self._loop.create_task(self._run_message_queue()),
        )

        self._log.debug(f"Scheduled {self._run_queues_task}")

    cpdef void _on_stop(self) except *:
        self.is_running = False
        self._data_queue.put_nowait(None)     # Sentinel message pattern
        self._message_queue.put_nowait(None)  # Sentinel message pattern
        self._log.debug(f"Sentinel message placed on data queue.")
        self._log.debug(f"Sentinel message placed on message queue.")

    async def _run_data_queue(self):
        self._log.debug(f"Data queue processing starting (qsize={self.data_qsize()})...")
        try:
            while self.is_running:
                data = await self._data_queue.get()
                if data is None:  # Sentinel message
                    continue      # Returns to the top to check `self.is_running`
                self._handle_data(data)
        except CancelledError:
            if self.data_qsize() > 0:
                self._log.warning(f"Running cancelled "
                                  f"with {self.data_qsize()} data item(s) on queue.")
            else:
                self._log.debug(f"Data queue processing stopped (qsize={self.data_qsize()}).")

    async def _run_message_queue(self):
        self._log.debug(f"Message queue processing starting (qsize={self.message_qsize()})...")
        cdef Message message
        try:
            while self.is_running:
                message = await self._message_queue.get()
                if message is None:  # Sentinel message
                    continue         # Returns to the top to check `self.is_running`
                if message.type == MessageType.COMMAND:
                    self._execute_command(message)
                elif message.type == MessageType.REQUEST:
                    self._handle_request(message)
                elif message.type == MessageType.RESPONSE:
                    self._handle_response(message)
                else:
                    self._log.error(f"Cannot handle unrecognized message {message}.")
        except CancelledError:
            if self.message_qsize() > 0:
                self._log.warning(f"Running cancelled "
                                  f"with {self.message_qsize()} message(s) on queue.")
            else:
                self._log.debug(f"Message queue processing stopped (qsize={self.message_qsize()}).")


cdef class LiveDataClient(DataClient):
    """
    The abstract base class for all live data clients.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        Venue venue not None,
        LiveDataEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `LiveDataClient` class.

        Parameters
        ----------
        venue : Venue
            The venue for the client.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        super().__init__(
            venue,
            engine,
            clock,
            logger,
        )

        self._loop: asyncio.AbstractEventLoop = engine.get_event_loop()
