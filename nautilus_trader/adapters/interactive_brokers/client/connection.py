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

from ibapi import comm
from ibapi import decoder
from ibapi.client import EClient
from ibapi.common import NO_VALID_ID
from ibapi.connection import Connection
from ibapi.errors import CONNECT_FAIL
from ibapi.server_versions import MAX_CLIENT_VER
from ibapi.server_versions import MIN_CLIENT_VER
from ibapi.utils import current_fn_name

from nautilus_trader.adapters.interactive_brokers.client.common import BaseMixin


class InteractiveBrokersClientConnectionMixin(BaseMixin):
    """
    Manages the connection to TWS/Gateway for the InteractiveBrokersClient.

    This class is responsible for establishing and maintaining the socket connection,
    handling server communication, monitoring the connection's health, and managing
    reconnections. When a connection is established, the `_is_ib_connected` event is set,
    and if the connection is lost, the `_is_ib_connected` event is cleared.

    """

    async def connect(self):
        """
        Establish the socket connection with TWS/Gateway. It initializes the connection,
        connects the socket, sends and receives version information, and then sets a
        flag that the connection has been successfully established.

        Raises
        ------
        Exception
            For any unexpected errors during the connection.

        """
        try:
            self._initialize_connection_params()
            await self._connect_socket()
            self._eclient.setConnState(EClient.CONNECTING)
            await self._send_version_info()
            self._eclient.decoder = decoder.Decoder(
                wrapper=self._eclient.wrapper,
                serverVersion=self._eclient.serverVersion(),
            )
            await self._receive_server_info()
            self._eclient.setConnState(EClient.CONNECTED)
            self._log.info(
                f"Connected to Interactive Brokers ({self._eclient.serverVersion_}) "
                f"at {self._eclient.connTime} from {self._host}:{self._port} "
                f"with client id: {self._client_id}.",
            )
            self._is_ib_connected.set()
        except Exception as e:
            self._log.error(f"Connection failed: {e}")
            await self._handle_reconnect()

    async def disconnect(self):
        try:
            self._eclient.disconnect()
            self._is_ib_connected.clear()
            self._log.info("Disconnected from Interactive Brokers API.")
        except Exception as e:
            self._log.error(f"Disconnection failed: {e}")

    async def _handle_reconnect(self):
        if self._reconnect_attempts < self._max_reconnect_attempts:
            self._reconnect_attempts += 1
            backoff_delay = self._reconnect_delay * (2 ** (self._reconnect_attempts - 1))
            self._log.info(
                f"Attempt {self._reconnect_attempts}: reconnecting in {backoff_delay:.2f} seconds...",
            )
            await asyncio.sleep(backoff_delay)
            await self.connect()
            self._reconnect_attempts = 0
        else:
            self._log.error("Max reconnection attempts reached. Connection failed.")

    async def _handle_connection_established(self):
        self._start_client_tasks_and_tws_api()
        self._get_account_ids()
        self._connection_watchdog_task = self._create_task(self._run_connection_watchdog())
        await self._subscribe_all()

    async def _handle_connection_lost(self):
        self._is_ib_connected.clear()
        await self._handle_reconnect()
        await self.resubscribe_all()

    def _initialize_connection_params(self) -> None:
        """
        Initialize the connection parameters before attempting to connect.

        Sets up the host, port, and client ID for the EClient instance and increments
        the connection attempt counter. Logs the attempt information.

        """
        self._eclient.reset()
        self._eclient._host = self._host
        self._eclient._port = self._port
        self._eclient.clientId = self._client_id
        self._connection_attempt_counter += 1
        self._log.info(
            f"Connecting to {self._host}:{self._port} with client id:{self._client_id}",
        )

    async def _connect_socket(self) -> None:
        """
        Connect the socket to TWS / Gateway and change the connection state to
        CONNECTING.

        It is an asynchronous method that runs within the event loop executor.

        """
        self._eclient.conn = Connection(self._host, self._port)
        await self._loop.run_in_executor(None, self._eclient.conn.connect)

    async def _send_version_info(self) -> None:
        """
        Send the API version information to TWS / Gateway.

        Constructs and sends a message containing the API version prefix and the version
        range supported by the client. This is part of the initial handshake process
        with the server.

        """
        v100prefix = "API\0"
        v100version = f"v{MIN_CLIENT_VER}..{MAX_CLIENT_VER}"
        if self._eclient.connectionOptions:
            v100version += f" {self._eclient.connectionOptions}"
        msg = comm.make_msg(v100version)
        msg2 = str.encode(v100prefix, "ascii") + msg
        await self._loop.run_in_executor(
            None,
            functools.partial(self._eclient.conn.sendMsg, msg2),
        )

    async def _receive_server_info(self) -> None:
        """
        Receive and process the server version information.

        Waits for the server to send its version information and connection time.
        Retries receiving this information up to a specified number of attempts.

        Raises
        ------
        ConnectionError
            If the server version information is not received within the allotted retries.

        """
        retries_remaining = 5
        fields: list[str] = []

        while retries_remaining > 0:
            buf = await self._loop.run_in_executor(None, self._eclient.conn.recvMsg)
            if len(buf) > 0:
                _, msg, _ = comm.read_msg(buf)
                fields.extend(comm.read_fields(msg))
            else:
                self._log.debug("Received empty buffer.")

            if len(fields) == 2:
                self._process_server_version(fields)
                break

            retries_remaining -= 1
            self._log.debug(
                "Failed to receive server version information."
                f"Retries remaining={retries_remaining}).",
            )
            await asyncio.sleep(1)

        if retries_remaining == 0:
            raise ConnectionError(
                "Max retry attempts reached. Failed to receive server version information.",
            )

    def _process_server_version(self, fields: list[str]) -> None:
        """
        Process and log the server version information. Extracts and sets the server
        version and connection time from the received fields. Logs the server version
        and connection time.

        Parameters
        ----------
        fields : list[str]
            The fields containing server version and connection time.

        """
        server_version, conn_time = int(fields[0]), fields[1]
        self._eclient.connTime = conn_time
        self._eclient.serverVersion_ = server_version
        self._eclient.decoder.serverVersion = server_version

    async def _probe_for_connectivity(self) -> None:
        """
        Perform a connectivity probe to TWS using a historical data request if the
        client is degraded.
        """
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

    def _handle_connection_error(self, e):
        """
        Handle any connection errors that occur during the connection setup. Logs the
        error, notifies the wrapper of the connection failure, and disconnects the
        client.

        Parameters
        ----------
        e : Exception
            The exception that occurred during the connection process.

        """
        if self._eclient.wrapper:
            self._eclient.wrapper.error(NO_VALID_ID, CONNECT_FAIL.code(), CONNECT_FAIL.msg())
        self._eclient.disconnect()
        self._log.error(f"Connection failed: {e}")

    # -- EWrapper overrides -----------------------------------------------------------------------
    def connectionClosed(self) -> None:
        """
        Indicate the API connection has closed.

        Following a API <-> TWS broken socket connection, this function is not called
        automatically but must be triggered by API client code.

        """
        self.logAnswer(current_fn_name(), vars())
        for future in self._requests.get_futures():
            if not future.done():
                future.set_exception(ConnectionError("Socket disconnected."))
        self._eclient.reset()
