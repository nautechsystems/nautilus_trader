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
Factories for creating Gate.io clients.
"""

from nautilus_trader.adapters.gateio2.config import GateioDataClientConfig
from nautilus_trader.adapters.gateio2.config import GateioExecClientConfig
from nautilus_trader.adapters.gateio2.providers import GateioInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.nautilus_pyo3 import GateioHttpClient
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


class GateioLiveDataClientFactory(LiveDataClientFactory):
    """
    Factory for creating Gate.io data clients.
    """

    @staticmethod
    def create(
        config: GateioDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ):
        """
        Create a Gate.io data client.

        Parameters
        ----------
        config : GateioDataClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        GateioDataClient
            The created data client.
        """
        # Import here to avoid circular dependency
        from nautilus_trader.adapters.gateio2.data import GateioDataClient

        # Create HTTP client
        http_client = GateioHttpClient(
            base_http_url=config.base_url_http,
            base_ws_spot_url=config.base_url_ws_spot,
            base_ws_futures_url=config.base_url_ws_futures,
            base_ws_options_url=config.base_url_ws_options,
            api_key=config.api_key,
            api_secret=config.api_secret,
        )

        # Create instrument provider
        instrument_provider = GateioInstrumentProvider(client=http_client)

        # Create data client
        return GateioDataClient(
            http_client=http_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )


class GateioLiveExecClientFactory(LiveExecClientFactory):
    """
    Factory for creating Gate.io execution clients.
    """

    @staticmethod
    def create(
        config: GateioExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ):
        """
        Create a Gate.io execution client.

        Parameters
        ----------
        config : GateioExecClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        GateioExecutionClient
            The created execution client.
        """
        # Import here to avoid circular dependency
        from nautilus_trader.adapters.gateio2.execution import GateioExecutionClient

        # Create HTTP client
        http_client = GateioHttpClient(
            base_http_url=config.base_url_http,
            base_ws_spot_url=config.base_url_ws_spot,
            base_ws_futures_url=config.base_url_ws_futures,
            base_ws_options_url=config.base_url_ws_options,
            api_key=config.api_key,
            api_secret=config.api_secret,
        )

        # Create instrument provider
        instrument_provider = GateioInstrumentProvider(client=http_client)

        # Create execution client
        return GateioExecutionClient(
            http_client=http_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )
