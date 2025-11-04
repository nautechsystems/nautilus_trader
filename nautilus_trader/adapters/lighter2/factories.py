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

from nautilus_trader.adapters.lighter2.config import LighterDataClientConfig
from nautilus_trader.adapters.lighter2.config import LighterExecClientConfig
from nautilus_trader.adapters.lighter2.data import LighterDataClient
from nautilus_trader.adapters.lighter2.execution import LighterExecutionClient
from nautilus_trader.adapters.lighter2.providers import LighterInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.core.nautilus_pyo3.lighter2 import LighterHttpClient
from nautilus_trader.core.nautilus_pyo3.lighter2 import LighterWebSocketClient
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory
from nautilus_trader.msgbus.bus import MessageBus


class LighterLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Lighter live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: LighterDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
    ) -> LighterDataClient:
        """
        Create a Lighter data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : LighterDataClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        Returns
        -------
        LighterDataClient

        """
        # Determine API key index (default to 2 if not provided)
        api_key_index = getattr(config, "api_key_index", None) or 2
        account_index = getattr(config, "account_index", None) or 1

        # Create Rust HTTP client
        http_client = LighterHttpClient(
            base_http_url=config.base_url_http,
            base_ws_url=config.base_url_ws,
            is_testnet=config.is_testnet,
            api_key_private_key=config.api_key_private_key,
            eth_private_key=config.eth_private_key,
            api_key_index=api_key_index,
            account_index=account_index,
        )

        # Create Rust WebSocket client
        ws_client = LighterWebSocketClient(
            base_http_url=config.base_url_http,
            base_ws_url=config.base_url_ws,
            is_testnet=config.is_testnet,
            api_key_private_key=config.api_key_private_key,
            eth_private_key=config.eth_private_key,
            api_key_index=api_key_index,
            account_index=account_index,
        )

        # Create instrument provider
        instrument_provider = LighterInstrumentProvider(
            client=http_client,
            logger=logger,
        )

        # Create data client
        return LighterDataClient(
            loop=loop,
            http_client=http_client,
            ws_client=ws_client,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )


class LighterLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a Lighter live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: LighterExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
    ) -> LighterExecutionClient:
        """
        Create a Lighter execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : LighterExecClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        Returns
        -------
        LighterExecutionClient

        """
        # Determine API key index (default to 2 if not provided)
        api_key_index = getattr(config, "api_key_index", None) or 2
        account_index = getattr(config, "account_index", None) or 1

        # Create Rust HTTP client
        http_client = LighterHttpClient(
            base_http_url=config.base_url_http,
            base_ws_url=config.base_url_ws,
            is_testnet=config.is_testnet,
            api_key_private_key=config.api_key_private_key,
            eth_private_key=config.eth_private_key,
            api_key_index=api_key_index,
            account_index=account_index,
        )

        # Create Rust WebSocket client
        ws_client = LighterWebSocketClient(
            base_http_url=config.base_url_http,
            base_ws_url=config.base_url_ws,
            is_testnet=config.is_testnet,
            api_key_private_key=config.api_key_private_key,
            eth_private_key=config.eth_private_key,
            api_key_index=api_key_index,
            account_index=account_index,
        )

        # Create execution client
        return LighterExecutionClient(
            loop=loop,
            http_client=http_client,
            ws_client=ws_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )
