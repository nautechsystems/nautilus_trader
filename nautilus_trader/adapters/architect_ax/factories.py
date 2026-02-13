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

import asyncio
from functools import lru_cache

from nautilus_trader.adapters.architect_ax.config import AxDataClientConfig
from nautilus_trader.adapters.architect_ax.config import AxExecClientConfig
from nautilus_trader.adapters.architect_ax.data import AxDataClient
from nautilus_trader.adapters.architect_ax.execution import AxExecutionClient
from nautilus_trader.adapters.architect_ax.providers import AxInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import AxEnvironment
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_cached_ax_http_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    base_url: str | None = None,
    orders_base_url: str | None = None,
    environment: AxEnvironment = AxEnvironment.SANDBOX,
    timeout_secs: int | None = None,
    max_retries: int | None = None,
    retry_delay_ms: int | None = None,
    retry_delay_max_ms: int | None = None,
    proxy_url: str | None = None,
) -> nautilus_pyo3.AxHttpClient:
    """
    Cache and return an AX Exchange HTTP client.

    If a cached client with matching parameters already exists, the cached client
    will be returned.

    Parameters
    ----------
    api_key : str, optional
        The API key for the client.
    api_secret : str, optional
        The API secret for the client.
    base_url : str, optional
        The base URL for the API endpoints.
    orders_base_url : str, optional
        The base URL for the orders API endpoints.
    environment : AxEnvironment, default AxEnvironment.SANDBOX
        The AX Exchange environment (Sandbox or Production).
    timeout_secs : int, optional
        The timeout for HTTP requests in seconds.
    max_retries : int, optional
        The maximum number of retry attempts for failed requests.
    retry_delay_ms : int, optional
        The initial delay (milliseconds) between retries.
    retry_delay_max_ms : int, optional
        The maximum delay (milliseconds) between retries.
    proxy_url : str, optional
        The proxy URL for HTTP requests.

    Returns
    -------
    AxHttpClient

    """
    if base_url is None:
        if environment == AxEnvironment.SANDBOX:
            base_url = "https://gateway.sandbox.architect.exchange/api"
        else:
            base_url = "https://gateway.architect.exchange/api"

    if orders_base_url is None:
        if environment == AxEnvironment.SANDBOX:
            orders_base_url = "https://gateway.sandbox.architect.exchange/orders"
        else:
            orders_base_url = "https://gateway.architect.exchange/orders"

    if api_key and api_secret:
        return nautilus_pyo3.AxHttpClient.with_credentials(
            api_key=api_key,
            api_secret=api_secret,
            base_url=base_url,
            orders_base_url=orders_base_url,
            timeout_secs=timeout_secs,
            max_retries=max_retries,
            retry_delay_ms=retry_delay_ms,
            retry_delay_max_ms=retry_delay_max_ms,
            proxy_url=proxy_url,
        )

    return nautilus_pyo3.AxHttpClient(
        base_url=base_url,
        orders_base_url=orders_base_url,
        timeout_secs=timeout_secs,
        max_retries=max_retries,
        retry_delay_ms=retry_delay_ms,
        retry_delay_max_ms=retry_delay_max_ms,
        proxy_url=proxy_url,
    )


@lru_cache(1)
def get_cached_ax_instrument_provider(
    client: nautilus_pyo3.AxHttpClient,
    config: InstrumentProviderConfig | None = None,
) -> AxInstrumentProvider:
    """
    Cache and return an AX Exchange instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : AxHttpClient
        The AX Exchange HTTP client.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    Returns
    -------
    AxInstrumentProvider

    """
    return AxInstrumentProvider(
        client=client,
        config=config,
    )


class AxLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides an AX Exchange live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: AxDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> AxDataClient:
        """
        Create a new AX Exchange data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : AxDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the instrument provider.

        Returns
        -------
        AxDataClient

        """
        client = get_cached_ax_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            base_url=config.base_url_http,
            environment=config.environment,
            timeout_secs=config.http_timeout_secs,
            max_retries=config.max_retries,
            retry_delay_ms=config.retry_delay_initial_ms,
            retry_delay_max_ms=config.retry_delay_max_ms,
            proxy_url=config.http_proxy_url,
        )
        provider = get_cached_ax_instrument_provider(
            client=client,
            config=config.instrument_provider,
        )
        return AxDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class AxLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides an AX Exchange live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: AxExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> AxExecutionClient:
        """
        Create a new AX Exchange execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : AxExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        AxExecutionClient

        """
        client = get_cached_ax_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            base_url=config.base_url_http,
            environment=config.environment,
            timeout_secs=config.http_timeout_secs,
            max_retries=config.max_retries,
            retry_delay_ms=config.retry_delay_initial_ms,
            retry_delay_max_ms=config.retry_delay_max_ms,
            proxy_url=config.http_proxy_url,
        )
        provider = get_cached_ax_instrument_provider(
            client=client,
            config=config.instrument_provider,
        )
        return AxExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
