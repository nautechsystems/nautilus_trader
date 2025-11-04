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
"""
Execution client for Gate.io.
"""

from nautilus_trader.adapters.gateio2.config import GateioExecClientConfig
from nautilus_trader.adapters.gateio2.providers import GateioInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.nautilus_pyo3 import GateioHttpClient
from nautilus_trader.live.execution_client import LiveExecutionClient


class GateioExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the Gate.io exchange.

    Parameters
    ----------
    http_client : GateioHttpClient
        The Rust HTTP client for Gate.io API.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : GateioInstrumentProvider
        The instrument provider for the client.
    config : GateioExecClientConfig
        The configuration for the client.
    """

    def __init__(
        self,
        http_client: GateioHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: GateioInstrumentProvider,
        config: GateioExecClientConfig,
    ) -> None:
        super().__init__(
            loop=msgbus.loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )

        self._http_client = http_client
        self._log.info("GateioExecutionClient initialized")

    async def _connect(self) -> None:
        """Connect to Gate.io execution streams."""
        self._log.info("Connecting to Gate.io execution...")
        # TODO: Implement WebSocket connection for order updates
        self._log.info("Connected to Gate.io execution")

    async def _disconnect(self) -> None:
        """Disconnect from Gate.io execution streams."""
        self._log.info("Disconnecting from Gate.io execution...")
        # TODO: Implement WebSocket disconnection
        self._log.info("Disconnected from Gate.io execution")
