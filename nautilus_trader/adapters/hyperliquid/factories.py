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

from __future__ import annotations

import asyncio
from functools import lru_cache
from typing import TYPE_CHECKING, Any

from nautilus_trader.adapters.hyperliquid.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid.config import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid.data import HyperliquidDataClient
from nautilus_trader.adapters.hyperliquid.execution import HyperliquidExecutionClient
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


if TYPE_CHECKING:
    pass  # TODO: Add imports for actual HyperliquidHttpClient when available


@lru_cache(1)
def get_cached_hyperliquid_http_client(
    private_key: str | None = None,
    vault_address: str | None = None,
    base_url: str | None = None,
    timeout_secs: int = 10,
    testnet: bool = False,
) -> Any:  # TODO: Replace with actual HyperliquidHttpClient when available
    """
    Cache and return a Hyperliquid HTTP client with the given parameters.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    private_key : str, optional
        The EVM private key for the client.
    vault_address : str, optional
        The vault address for vault trading.
    base_url : str, optional
        The base URL for the API endpoints.
    timeout_secs : int, default 10
        The timeout (seconds) for HTTP requests to Hyperliquid.
    testnet : bool, default False
        If the client is connecting to the testnet API.

    Returns
    -------
    Any
        Placeholder for HyperliquidHttpClient

    """

    # TODO: Implement actual HyperliquidHttpClient instantiation
    # This is a placeholder that returns a mock client
    class MockHyperliquidHttpClient:
        def __init__(self, **kwargs):
            self.params = kwargs

    return MockHyperliquidHttpClient(
        private_key=private_key,
        vault_address=vault_address,
        base_url=base_url,
        timeout_secs=timeout_secs,
        testnet=testnet,
    )


@lru_cache(1)
def get_cached_hyperliquid_instrument_provider(
    client: Any,  # TODO: Replace with actual HyperliquidHttpClient when available
    config: InstrumentProviderConfig | None = None,
) -> HyperliquidInstrumentProvider:
    """
    Cache and return a Hyperliquid instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : Any
        The Hyperliquid HTTP client (placeholder).
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    Returns
    -------
    HyperliquidInstrumentProvider

    """
    return HyperliquidInstrumentProvider(
        client=client,
        config=config,
    )


class HyperliquidLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Hyperliquid live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: HyperliquidDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> HyperliquidDataClient:
        """
        Create a new Hyperliquid data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : HyperliquidDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock: LiveClock
            The clock for the instrument provider.

        Returns
        -------
        HyperliquidDataClient

        """
        client = get_cached_hyperliquid_http_client(
            base_url=config.base_url_http,
            timeout_secs=config.http_timeout_secs,
            testnet=config.testnet,
        )
        provider = get_cached_hyperliquid_instrument_provider(
            client=client,
            config=config.instrument_provider,
        )
        return HyperliquidDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class HyperliquidLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a Hyperliquid live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: HyperliquidExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> HyperliquidExecutionClient:
        """
        Create a new Hyperliquid execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : HyperliquidExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        HyperliquidExecutionClient

        """
        client = get_cached_hyperliquid_http_client(
            private_key=config.private_key,
            vault_address=config.vault_address,
            base_url=config.base_url_http,
            timeout_secs=config.http_timeout_secs,
            testnet=config.testnet,
        )
        provider = get_cached_hyperliquid_instrument_provider(
            client=client,
            config=config.instrument_provider,
        )
        return HyperliquidExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
