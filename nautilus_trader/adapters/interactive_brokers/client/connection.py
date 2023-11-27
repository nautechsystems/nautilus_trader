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
import functools
from inspect import iscoroutinefunction

from ibapi import comm
from ibapi.client import EClient
from ibapi.common import NO_VALID_ID
from ibapi.connection import Connection
from ibapi.errors import CONNECT_FAIL
from ibapi.server_versions import MAX_CLIENT_VER
from ibapi.server_versions import MIN_CLIENT_VER
from ibapi.utils import current_fn_name
from ibapi.wrapper import EWrapper

# fmt: off
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import instrument_id_to_ib_contract
from nautilus_trader.model.identifiers import InstrumentId


# fmt: on


class InteractiveBrokersConnectionManager(EWrapper):
    """
    Manages the connection to TWS/Gateway for the InteractiveBrokersClient.

    This class is responsible for establishing and maintaining the socket connection,
    handling server communication, monitoring the connection's health, and managing
    reconnections.

    """

    def __init__(self, client):
        self._client = client
        self._eclient: EClient = client._eclient
        self._log = client._log
        self.host: str = client.host
        self.port: int = client.port

        self._connection_attempt_counter = 0
        self._contract_for_probe = instrument_id_to_ib_contract(
            InstrumentId.from_str("EUR/CHF.IDEALPRO"),
        )

    async def _socket_connect(self) -> None:
        """
        Establish the socket connection with TWS/Gateway. It initializes the connection,
        connects the socket, sends and receives version information, and then sets up
        the client.

        Returns
        -------
        None

        Raises
        ------
        OSError
            If an OSError occurs during the connection process.
        Exception
            For any other unexpected errors during the connection.

        """
        self._initialize_connection()
        try:
            await self._connect_socket()
            await self._send_version_info()
            await self._receive_server_info()
            self._setup_client()
            self._log.debug("Connection established successfully.")
        except OSError as e:
            self._handle_connection_error(e)
        except Exception as e:
            self._log.exception("Unexpected error during connection", e)

    def _initialize_connection(self) -> None:
        """
        Initialize the connection parameters before attempting to connect.

        Sets up the host, port, and client ID for the EClient instance and
        increments the connection attempt counter. Logs the attempt information.

        Returns
        -------
        None

        """
        self._eclient.host = self._client.host
        self._eclient.port = self._client.port
        self._eclient.clientId = self._client.client_id
        self._connection_attempt_counter += 1
        self._log.info(
            f"Attempt {self._connection_attempt_counter}: "
            f"Connecting to {self.host}:{self.port} w/ id:{self._client.client_id}",
        )

    async def _connect_socket(self) -> None:
        """
        Connect the socket to TWS / Gateway and change the connection state to
        CONNECTING. It is an asynchronous method that runs within the event loop
        executor.

        Returns
        -------
        None

        """
        self._eclient.conn = Connection(self.host, self.port)
        await self._client.loop.run_in_executor(None, self._eclient.conn.connect)
        self._eclient.setConnState(EClient.CONNECTING)

    async def _send_version_info(self):
        """
        Send the API version information to TWS / Gateway.

        Constructs and sends a message containing the API version prefix
        and the version range supported by the client. This is part of
        the initial handshake process with the server.

        Returns
        -------
        Any

        """
        v100prefix = "API\0"
        v100version = "v%d..%d" % (MIN_CLIENT_VER, MAX_CLIENT_VER)
        if self._eclient.connectionOptions:
            v100version += f" {self._eclient.connectionOptions}"
        msg = comm.make_msg(v100version)
        msg2 = str.encode(v100prefix, "ascii") + msg
        await self._client.loop.run_in_executor(
            None,
            functools.partial(self._eclient.conn.sendMsg, msg2),
        )

    async def _receive_server_info(self) -> None:
        """
        Receive and process the server version information.

        Waits for the server to send its version information and connection time.
        Retries receiving this information up to a specified number of attempts.

        Returns
        -------
        None

        Raises
        ------
        ConnectionError
            If the server version information is not received within the allotted retries.

        """
        connection_retries_remaining = 5
        fields: list[str] = []

        while len(fields) != 2 and connection_retries_remaining > 0:
            await asyncio.sleep(1)
            buf = await self._client.loop.run_in_executor(None, self._eclient.conn.recvMsg)
            self._process_received_buffer(buf, connection_retries_remaining, fields)

        if len(fields) == 2:
            self._process_server_version(fields)
        else:
            raise ConnectionError("Failed to receive server version information.")

    def _process_received_buffer(self, buf, retries_remaining, fields) -> None:
        """
        Process the received buffer from TWS API. Reads the received message and
        extracts fields from it. Handles situations where the connection might be lost
        or the received buffer is empty.

        Parameters
        ----------
        buf : bytes
            The received buffer from the server.
        retries_remaining : int
            The number of remaining retries for receiving the message.
        fields : list[str]
            The list to which the extracted fields will be appended.

        Returns
        -------
        None

        """
        if not self._eclient.conn.isConnected() or retries_remaining <= 0:
            self._log.warning("Disconnected; resetting connection")
            self._client.reset()
            return
        if len(buf) > 0:
            _, msg, _ = comm.read_msg(buf)
            fields.extend(comm.read_fields(msg))
        else:
            self._log.debug(f"Received empty buffer (retries_remaining={retries_remaining})")

    def _process_server_version(self, fields) -> None:
        """
        Process and log the server version information. Extracts and sets the server
        version and connection time from the received fields. Logs the server version
        and connection time.

        Parameters
        ----------
        fields : list[str]
            The fields containing server version and connection time.

        Returns
        -------
        None

        """
        server_version, conn_time = int(fields[0]), fields[1]
        self._eclient.connTime = conn_time
        self._eclient.serverVersion_ = server_version
        self._eclient.decoder.serverVersion = server_version
        self._log.debug(f"Connected to server version {server_version} at {conn_time}")

    def _setup_client(self) -> None:
        """
        Set up the client after a successful connection. Changes the client state to
        CONNECTED, starts the incoming message reader and queue tasks, and initiates the
        start API call to the EClient.

        Returns
        -------
        None

        """
        self._client.setConnState(EClient.CONNECTED)
        if self._client.tws_incoming_msg_reader_task:
            self._client.tws_incoming_msg_reader_task.cancel()
        self._client.tws_incoming_msg_reader_task = self._client.create_task(
            self._client.run_tws_incoming_msg_reader(),
        )
        self._client.internal_msg_queue_task = self._client.create_task(
            self._client.run_internal_msg_queue(),
        )
        self._eclient.startApi()

    def _handle_connection_error(self, e):
        """
        Handle any connection errors that occur during the connection setup. Logs the
        error, notifies the wrapper of the connection failure, and disconnects the
        client.

        Parameters
        ----------
        e : Exception
            The exception that occurred during the connection process.

        Returns
        -------
        None

        """
        if self._eclient.wrapper:
            self._eclient.wrapper.error(NO_VALID_ID, CONNECT_FAIL.code(), CONNECT_FAIL.msg())
        self._eclient.disconnect()
        self._log.error("Connection failed", e)

    async def run_watch_dog(self):
        """
        Run a watchdog to monitor and manage the health of the socket connection.
        Continuously checks the connection status, manages client state based on
        connection health, and handles subscription management in case of network
        failure or IB nightly reset.

        Returns
        -------
        None

        Raises
        ------
        asyncio.CancelledError
            If the watchdog task gets cancelled.

        """
        try:
            while True:
                await asyncio.sleep(1)
                if self._eclient.isConnected():
                    if self._client.is_ib_ready.is_set():
                        await self._handle_ib_is_ready()
                    else:
                        await self._handle_ib_is_not_ready()
                else:
                    await self._handle_socket_connectivity()
        except asyncio.CancelledError:
            self._log.debug("`watch_dog` task was canceled.")

    async def _handle_ib_is_ready(self) -> None:
        """
        Handle actions when Interactive Brokers is ready and the connection is fully
        functional. If the client state is degraded, it attempts to cancel and restart
        all subscriptions. If the client has been initialized but not yet running, it
        starts the client.

        Returns
        -------
        None

        Raises
        ------
        Exception
            If an error occurs during the handling of subscriptions or starting the client.

        """
        if self._client.is_degraded:
            for subscription in self._client.subscriptions.get_all():
                try:
                    subscription.cancel()
                    if iscoroutinefunction(subscription.handle):
                        await subscription.handle()
                    else:
                        await self._client.loop.run_in_executor(None, subscription.handle)
                except Exception as e:
                    self._log.exception("failed subscription", e)
            self._client.resume()
        elif self._client.is_initialized and not self._client.is_running:
            self._client.start()

    async def _handle_ib_is_not_ready(self):
        """
        Manage actions when Interactive Brokers is not ready or the connection is
        degraded. Performs a connectivity probe to TWS using a historical data request
        if the client is degraded. If the client is running, it handles the situation
        where connectivity between TWS/Gateway and IB server is broken.

        Returns
        -------
        None

        Raises
        ------
        Exception
            If an error occurs during the probe or handling degraded state.

        """
        if self._client.is_degraded:
            # Probe connectivity. Sometime restored event will not be received from TWS without this
            self._eclient.reqHistoricalData(
                reqId=1,
                contract=self._contract_for_probe,
                endDateTime="",
                durationStr="30 S",
                barSizeSetting="5 secs",
                whatToShow="MIDPOINT",
                useRTH=False,
                formatDate=2,
                keepUpToDate=False,
                chartOptions=[],
            )
            await asyncio.sleep(15)
            self._eclient.cancelHistoricalData(1)
        elif self._client.is_running:
            # Connectivity between TWS/Gateway and IB server is broken
            if self._client.is_running:
                self._client.degrade()

    async def _handle_socket_connectivity(self):
        """
        Manage socket connectivity, including reconnection attempts and error handling.
        Degrades the client if it's currently running and tries to re-establish the
        socket connection. Waits for the Interactive Brokers readiness signal, logging
        success or failure accordingly.

        Raises
        ------
        asyncio.TimeoutError
            If the connection attempt times out.
        Exception
            For general failures in re-establishing the connection.

        """
        if self._client.is_running:
            self._client.degrade()
        self._client.is_ib_ready.clear()
        await asyncio.sleep(5)  # Avoid too fast attempts
        await self._socket_connect()
        try:
            await asyncio.wait_for(self._client.is_ib_ready.wait(), 15)
            self._log.info(
                f"Connected to {self.host}:{self.port} w/ id:{self._client.client_id}",
            )
        except asyncio.TimeoutError:
            self._log.error(
                f"Unable to connect {self.host}:{self.port} w/ id:{self._client.client_id}",
            )
        except Exception as e:
            self._log.exception("Failed connection", e)

    # -- EWrapper overrides -----------------------------------------------------------------------
    def connectionClosed(self) -> None:
        """
        Indicate the API connection has closed. Following a API <-> TWS broken socket
        connection, this function is not called automatically but must be triggered by
        API client code.

        Returns
        -------
        None

        """
        self._client.logAnswer(current_fn_name(), vars())
        error = ConnectionError("Socket disconnect")
        for future in self._client.requests.get_futures():
            if not future.done():
                future.set_exception(error)
        self._eclient.reset()
