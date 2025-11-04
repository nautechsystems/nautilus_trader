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
Factory functions for creating Asterdex clients.
"""

import os

from nautilus_trader.adapters.asterdex2.config import AsterdexDataClientConfig
from nautilus_trader.adapters.asterdex2.config import AsterdexExecClientConfig
from nautilus_trader.adapters.asterdex2.config import get_env_key
from nautilus_trader.adapters.asterdex2.data import AsterdexLiveDataClient
from nautilus_trader.adapters.asterdex2.execution import AsterdexLiveExecClient
from nautilus_trader.adapters.asterdex2.http.client import AsterdexHttpClient
from nautilus_trader.adapters.asterdex2.providers import AsterdexInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


def get_cached_asterdex_http_client(
    config: AsterdexDataClientConfig | AsterdexExecClientConfig,
    cache: Cache,
) -> AsterdexHttpClient:
    """
    Cache and return an Asterdex HTTP client instance.

    If a cached client with matching key exists, then that cached client will be returned.

    Parameters
    ----------
    config : AsterdexDataClientConfig | AsterdexExecClientConfig
        The configuration for the client.
    cache : Cache
        The cache instance.

    Returns
    -------
    AsterdexHttpClient

    """
    # Get credentials from config or environment
    api_key = config.api_key or os.getenv(get_env_key("api_key"))
    api_secret = config.api_secret or os.getenv(get_env_key("api_secret"))

    # Create a cache key from config
    key = (
        api_key,
        api_secret,
        config.base_url_http_spot,
        config.base_url_http_futures,
    )

    # Check if client already exists in cache
    if key in cache._asterdex_http_clients:
        return cache._asterdex_http_clients[key]

    # Create new HTTP client
    http_client = AsterdexHttpClient(
        base_url_http_spot=config.base_url_http_spot,
        base_url_http_futures=config.base_url_http_futures,
        api_key=api_key,
        api_secret=api_secret,
    )

    # Cache the client
    if not hasattr(cache, "_asterdex_http_clients"):
        cache._asterdex_http_clients = {}
    cache._asterdex_http_clients[key] = http_client

    return http_client


def get_cached_asterdex_instrument_provider(
    config: AsterdexDataClientConfig | AsterdexExecClientConfig,
    cache: Cache,
) -> AsterdexInstrumentProvider:
    """
    Cache and return an Asterdex instrument provider instance.

    If a cached provider with matching key exists, then that cached provider will be returned.

    Parameters
    ----------
    config : AsterdexDataClientConfig | AsterdexExecClientConfig
        The configuration for the provider.
    cache : Cache
        The cache instance.

    Returns
    -------
    AsterdexInstrumentProvider

    """
    # Get HTTP client (cached)
    http_client = get_cached_asterdex_http_client(config, cache)

    # Create cache key
    key = id(http_client)

    # Check if provider already exists in cache
    if key in getattr(cache, "_asterdex_instrument_providers", {}):
        return cache._asterdex_instrument_providers[key]

    # Create new provider
    provider = AsterdexInstrumentProvider(client=http_client)

    # Cache the provider
    if not hasattr(cache, "_asterdex_instrument_providers"):
        cache._asterdex_instrument_providers = {}
    cache._asterdex_instrument_providers[key] = provider

    return provider


class AsterdexLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a `AsterdexLiveDataClient` factory.
    """

    @staticmethod
    def create(
        config: AsterdexDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> AsterdexLiveDataClient:
        """
        Create a new Asterdex data client.

        Parameters
        ----------
        config : AsterdexDataClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        AsterdexLiveDataClient

        """
        http_client = get_cached_asterdex_http_client(config, cache)
        instrument_provider = get_cached_asterdex_instrument_provider(config, cache)

        return AsterdexLiveDataClient(
            loop=clock.get_event_loop(),
            http_client=http_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )


class AsterdexLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a `AsterdexLiveExecClient` factory.
    """

    @staticmethod
    def create(
        config: AsterdexExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> AsterdexLiveExecClient:
        """
        Create a new Asterdex execution client.

        Parameters
        ----------
        config : AsterdexExecClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        AsterdexLiveExecClient

        """
        http_client = get_cached_asterdex_http_client(config, cache)
        instrument_provider = get_cached_asterdex_instrument_provider(config, cache)

        return AsterdexLiveExecClient(
            loop=clock.get_event_loop(),
            http_client=http_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )
