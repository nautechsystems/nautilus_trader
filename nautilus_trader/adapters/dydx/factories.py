# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
Factories for creating dYdX v4 data and execution clients.
"""

import asyncio
from functools import lru_cache

from nautilus_trader.adapters.dydx.config import DydxDataClientConfig
from nautilus_trader.adapters.dydx.config import DydxExecClientConfig
from nautilus_trader.adapters.dydx.data import DydxDataClient
from nautilus_trader.adapters.dydx.execution import DydxExecutionClient
from nautilus_trader.adapters.dydx.providers import DydxInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_cached_dydx_http_client(
    base_url: str | None = None,
    is_testnet: bool = False,
) -> nautilus_pyo3.DydxHttpClient:  # type: ignore[name-defined]
    """
    Cache and return a dYdX HTTP client.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    base_url : str, optional
        The base URL for the API endpoints.
    is_testnet : bool, default False
        If the client is for the dYdX testnet API.

    Returns
    -------
    DydxHttpClient

    """
    return nautilus_pyo3.DydxHttpClient(  # type: ignore[attr-defined]
        base_url=base_url,
        is_testnet=is_testnet,
    )


@lru_cache(1)
def get_cached_dydx_instrument_provider(
    client: nautilus_pyo3.DydxHttpClient,  # type: ignore[name-defined]
    config: InstrumentProviderConfig | None = None,
) -> DydxInstrumentProvider:
    """
    Cache and return a dYdX instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : DydxHttpClient
        The dYdX HTTP client.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration.

    Returns
    -------
    DydxInstrumentProvider

    """
    return DydxInstrumentProvider(
        client=client,
        config=config,
    )


class DydxLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a dYdX v4 live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: DydxDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> DydxDataClient:
        """
        Create a new dYdX v4 data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : DydxDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        DydxDataClient

        """
        client: nautilus_pyo3.DydxHttpClient = get_cached_dydx_http_client(  # type: ignore[name-defined]
            base_url=config.base_url_http,
            is_testnet=config.is_testnet,
        )
        provider = get_cached_dydx_instrument_provider(
            client=client,
            config=config.instrument_provider,
        )
        return DydxDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class DydxLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a dYdX v4 live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: DydxExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> DydxExecutionClient:
        """
        Create a new dYdX v4 execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : DydxExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        DydxExecutionClient

        """
        client: nautilus_pyo3.DydxHttpClient = get_cached_dydx_http_client(  # type: ignore[name-defined]
            base_url=config.base_url_http,
            is_testnet=config.is_testnet,
        )
        provider = get_cached_dydx_instrument_provider(
            client=client,
            config=config.instrument_provider,
        )
        return DydxExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
