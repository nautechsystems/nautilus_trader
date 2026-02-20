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

from nautilus_trader.adapters.deribit.config import DeribitDataClientConfig
from nautilus_trader.adapters.deribit.config import DeribitExecClientConfig
from nautilus_trader.adapters.deribit.data import DeribitDataClient
from nautilus_trader.adapters.deribit.execution import DeribitExecutionClient
from nautilus_trader.adapters.deribit.providers import DeribitInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import DeribitProductType
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_cached_deribit_http_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    base_url: str | None = None,
    is_testnet: bool = False,
    timeout_secs: int | None = None,
    max_retries: int | None = None,
    retry_delay_ms: int | None = None,
    retry_delay_max_ms: int | None = None,
) -> nautilus_pyo3.DeribitHttpClient:
    """
    Cache and return a Deribit HTTP client with the given key and secret.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    api_key : str, optional
        The API key for the client.
    api_secret : str, optional
        The API secret for the client.
    base_url : str, optional
        The base URL for the API endpoints.
    is_testnet : bool, default False
        If the client is for the Deribit testnet API.
    timeout_secs : int, optional
        The timeout (seconds) for HTTP requests to Deribit.
    max_retries : int, optional
        The maximum retry attempts for requests.
    retry_delay_ms : int, optional
        The initial delay (milliseconds) between retries.
    retry_delay_max_ms : int, optional
        The maximum delay (milliseconds) between retries.

    Returns
    -------
    DeribitHttpClient

    """
    return nautilus_pyo3.DeribitHttpClient(
        api_key=api_key,
        api_secret=api_secret,
        base_url=base_url,
        is_testnet=is_testnet,
        timeout_secs=timeout_secs,
        max_retries=max_retries,
        retry_delay_ms=retry_delay_ms,
        retry_delay_max_ms=retry_delay_max_ms,
    )


@lru_cache(1)
def get_cached_deribit_instrument_provider(
    client: nautilus_pyo3.DeribitHttpClient,
    product_types: tuple[DeribitProductType, ...] | None = None,
    config: InstrumentProviderConfig | None = None,
) -> DeribitInstrumentProvider:
    """
    Cache and return a Deribit instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    client : DeribitHttpClient
        The Deribit HTTP client.
    product_types : tuple[DeribitProductType, ...], optional
        The product types to load.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    Returns
    -------
    DeribitInstrumentProvider

    """
    return DeribitInstrumentProvider(
        client=client,
        product_types=product_types,
        config=config,
    )


class DeribitLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Deribit live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: DeribitDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> DeribitDataClient:
        """
        Create a new Deribit data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : DeribitDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock: LiveClock
            The clock for the instrument provider.

        Returns
        -------
        DeribitDataClient

        """
        client: nautilus_pyo3.DeribitHttpClient = get_cached_deribit_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            base_url=config.base_url_http,
            is_testnet=config.is_testnet,
            timeout_secs=config.http_timeout_secs,
            max_retries=config.max_retries,
            retry_delay_ms=config.retry_delay_initial_ms,
            retry_delay_max_ms=config.retry_delay_max_ms,
        )
        provider = get_cached_deribit_instrument_provider(
            client=client,
            product_types=config.product_types,
            config=config.instrument_provider,
        )
        return DeribitDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class DeribitLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a Deribit live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: DeribitExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> DeribitExecutionClient:
        """
        Create a new Deribit execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : DeribitExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock: LiveClock
            The clock for the instrument provider.

        Returns
        -------
        DeribitExecutionClient

        """
        http_client: nautilus_pyo3.DeribitHttpClient = get_cached_deribit_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            base_url=config.base_url_http,
            is_testnet=config.is_testnet,
            timeout_secs=config.http_timeout_secs,
            max_retries=config.max_retries,
            retry_delay_ms=config.retry_delay_initial_ms,
            retry_delay_max_ms=config.retry_delay_max_ms,
        )

        provider = get_cached_deribit_instrument_provider(
            client=http_client,
            product_types=config.product_types,
            config=config.instrument_provider,
        )
        return DeribitExecutionClient(
            loop=loop,
            http_client=http_client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
