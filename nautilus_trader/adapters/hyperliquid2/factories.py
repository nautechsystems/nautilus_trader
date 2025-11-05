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
Factory functions for creating Hyperliquid clients.
"""

import os

from nautilus_hyperliquid2 import Hyperliquid2HttpClient
from nautilus_hyperliquid2 import Hyperliquid2WebSocketClient

from nautilus_trader.adapters.hyperliquid2.config import Hyperliquid2DataClientConfig
from nautilus_trader.adapters.hyperliquid2.config import Hyperliquid2ExecClientConfig
from nautilus_trader.adapters.hyperliquid2.data import Hyperliquid2LiveDataClient
from nautilus_trader.adapters.hyperliquid2.execution import Hyperliquid2LiveExecClient
from nautilus_trader.adapters.hyperliquid2.providers import Hyperliquid2InstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


def get_env_key(key: str) -> str:
    """Get environment variable key for Hyperliquid configuration."""
    return f"HYPERLIQUID_{key.upper()}"


def get_cached_hyperliquid2_http_client(
    config: Hyperliquid2DataClientConfig | Hyperliquid2ExecClientConfig,
    cache: Cache,
) -> Hyperliquid2HttpClient:
    """
    Get or create a cached Hyperliquid HTTP client.

    Parameters
    ----------
    config : Hyperliquid2DataClientConfig | Hyperliquid2ExecClientConfig
        The configuration for the client.
    cache : Cache
        The cache for storing clients.

    Returns
    -------
    Hyperliquid2HttpClient

    """
    # Get configuration values with environment variable fallback
    private_key = config.private_key or os.getenv(get_env_key("private_key"))
    http_base = config.http_base
    testnet = config.testnet

    # Create cache key
    cache_key = (private_key, http_base, testnet)

    # Check if client exists in cache
    if not hasattr(cache, "_hyperliquid2_http_clients"):
        cache._hyperliquid2_http_clients = {}

    if cache_key in cache._hyperliquid2_http_clients:
        return cache._hyperliquid2_http_clients[cache_key]

    # Create new HTTP client
    http_client = Hyperliquid2HttpClient(
        private_key=private_key,
        http_base=http_base,
        testnet=testnet,
    )

    # Cache the client
    cache._hyperliquid2_http_clients[cache_key] = http_client

    return http_client


def get_cached_hyperliquid2_ws_client(
    config: Hyperliquid2DataClientConfig | Hyperliquid2ExecClientConfig,
    cache: Cache,
) -> Hyperliquid2WebSocketClient:
    """
    Get or create a cached Hyperliquid WebSocket client.

    Parameters
    ----------
    config : Hyperliquid2DataClientConfig | Hyperliquid2ExecClientConfig
        The configuration for the client.
    cache : Cache
        The cache for storing clients.

    Returns
    -------
    Hyperliquid2WebSocketClient

    """
    # Get configuration values
    ws_base = config.ws_base
    testnet = config.testnet

    # Create cache key
    cache_key = (ws_base, testnet)

    # Check if client exists in cache
    if not hasattr(cache, "_hyperliquid2_ws_clients"):
        cache._hyperliquid2_ws_clients = {}

    if cache_key in cache._hyperliquid2_ws_clients:
        return cache._hyperliquid2_ws_clients[cache_key]

    # Create new WebSocket client
    ws_client = Hyperliquid2WebSocketClient(
        ws_base=ws_base,
        testnet=testnet,
    )

    # Cache the client
    cache._hyperliquid2_ws_clients[cache_key] = ws_client

    return ws_client


def get_cached_hyperliquid2_instrument_provider(
    config: Hyperliquid2DataClientConfig | Hyperliquid2ExecClientConfig,
    cache: Cache,
) -> Hyperliquid2InstrumentProvider:
    """
    Get or create a cached Hyperliquid instrument provider.

    Parameters
    ----------
    config : Hyperliquid2DataClientConfig | Hyperliquid2ExecClientConfig
        The configuration for the provider.
    cache : Cache
        The cache for storing providers.

    Returns
    -------
    Hyperliquid2InstrumentProvider

    """
    http_client = get_cached_hyperliquid2_http_client(config, cache)

    # Create cache key
    cache_key = id(http_client)

    # Check if provider exists in cache
    if not hasattr(cache, "_hyperliquid2_instrument_providers"):
        cache._hyperliquid2_instrument_providers = {}

    if cache_key in cache._hyperliquid2_instrument_providers:
        return cache._hyperliquid2_instrument_providers[cache_key]

    # Create new instrument provider
    provider = Hyperliquid2InstrumentProvider(client=http_client)

    # Cache the provider
    cache._hyperliquid2_instrument_providers[cache_key] = provider

    return provider


class Hyperliquid2LiveDataClientFactory(LiveDataClientFactory):
    """
    Factory for creating Hyperliquid2 live data clients.
    """

    @staticmethod
    def create(
        config: Hyperliquid2DataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> Hyperliquid2LiveDataClient:
        """
        Create a Hyperliquid2 live data client.

        Parameters
        ----------
        config : Hyperliquid2DataClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        Hyperliquid2LiveDataClient

        """
        http_client = get_cached_hyperliquid2_http_client(config, cache)
        ws_client = get_cached_hyperliquid2_ws_client(config, cache)
        provider = get_cached_hyperliquid2_instrument_provider(config, cache)

        return Hyperliquid2LiveDataClient(
            http_client=http_client,
            ws_client=ws_client,
            instrument_provider=provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )


class Hyperliquid2LiveExecClientFactory(LiveExecClientFactory):
    """
    Factory for creating Hyperliquid2 live execution clients.
    """

    @staticmethod
    def create(
        config: Hyperliquid2ExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> Hyperliquid2LiveExecClient:
        """
        Create a Hyperliquid2 live execution client.

        Parameters
        ----------
        config : Hyperliquid2ExecClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        Hyperliquid2LiveExecClient

        """
        http_client = get_cached_hyperliquid2_http_client(config, cache)
        ws_client = get_cached_hyperliquid2_ws_client(config, cache)
        provider = get_cached_hyperliquid2_instrument_provider(config, cache)

        return Hyperliquid2LiveExecClient(
            http_client=http_client,
            ws_client=ws_client,
            instrument_provider=provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )
