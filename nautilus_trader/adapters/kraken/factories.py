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
from functools import lru_cache

from nautilus_trader.adapters.kraken.config import KrakenDataClientConfig
from nautilus_trader.adapters.kraken.config import KrakenExecClientConfig
from nautilus_trader.adapters.kraken.data import KrakenDataClient
from nautilus_trader.adapters.kraken.execution import KrakenExecutionClient
from nautilus_trader.adapters.kraken.providers import KrakenInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import KrakenEnvironment
from nautilus_trader.core.nautilus_pyo3 import KrakenProductType
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(1)
def get_cached_kraken_spot_http_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    base_url: str | None = None,
    demo: bool = False,
    timeout_secs: int | None = None,
    max_retries: int | None = None,
    retry_delay_ms: int | None = None,
    retry_delay_max_ms: int | None = None,
    proxy_url: str | None = None,
    max_requests_per_second: int | None = None,
) -> nautilus_pyo3.KrakenSpotHttpClient:
    """
    Cache and return a Kraken Spot HTTP client.

    If ``api_key`` and ``api_secret`` are ``None``, then they will be sourced from the
    environment variables ``KRAKEN_SPOT_API_KEY`` and ``KRAKEN_SPOT_API_SECRET``.

    Note: Kraken Spot does not have a testnet/demo environment.

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    api_key : str, optional
        The Kraken API key for the client.
    api_secret : str, optional
        The Kraken API secret for the client.
    base_url : str, optional
        The base URL for the Kraken Spot API.
    demo : bool, default False
        Unused for Spot (Kraken Spot has no demo environment).
    timeout_secs : int, optional
        The timeout in seconds for HTTP requests.
    max_retries : int, optional
        The maximum number of retry attempts for failed requests.
    retry_delay_ms : int, optional
        The initial delay in milliseconds between retry attempts.
    retry_delay_max_ms : int, optional
        The maximum delay in milliseconds between retry attempts.
    proxy_url : str, optional
        The proxy URL for HTTP requests.
    max_requests_per_second : int, optional
        The maximum number of requests per second for rate limiting.

    Returns
    -------
    nautilus_pyo3.KrakenSpotHttpClient

    """
    return nautilus_pyo3.KrakenSpotHttpClient(
        api_key=api_key,
        api_secret=api_secret,
        base_url=base_url,
        demo=demo,
        timeout_secs=timeout_secs,
        max_retries=max_retries,
        retry_delay_ms=retry_delay_ms,
        retry_delay_max_ms=retry_delay_max_ms,
        proxy_url=proxy_url,
        max_requests_per_second=max_requests_per_second,
    )


@lru_cache(1)
def get_cached_kraken_futures_http_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    base_url: str | None = None,
    demo: bool = False,
    timeout_secs: int | None = None,
    max_retries: int | None = None,
    retry_delay_ms: int | None = None,
    retry_delay_max_ms: int | None = None,
    proxy_url: str | None = None,
    max_requests_per_second: int | None = None,
) -> nautilus_pyo3.KrakenFuturesHttpClient:
    """
    Cache and return a Kraken Futures HTTP client.

    If ``api_key`` and ``api_secret`` are ``None``, then they will be sourced from the
    environment variables ``KRAKEN_FUTURES_API_KEY`` and ``KRAKEN_FUTURES_API_SECRET``
    (or ``KRAKEN_FUTURES_DEMO_API_KEY`` and ``KRAKEN_FUTURES_DEMO_API_SECRET``
    for the demo environment).

    If a cached client with matching parameters already exists, the cached client will be returned.

    Parameters
    ----------
    api_key : str, optional
        The Kraken API key for the client.
    api_secret : str, optional
        The Kraken API secret for the client.
    base_url : str, optional
        The base URL for the Kraken Futures API.
    demo : bool, default False
        If True, use demo environment variables for credentials.
    timeout_secs : int, optional
        The timeout in seconds for HTTP requests.
    max_retries : int, optional
        The maximum number of retry attempts for failed requests.
    retry_delay_ms : int, optional
        The initial delay in milliseconds between retry attempts.
    retry_delay_max_ms : int, optional
        The maximum delay in milliseconds between retry attempts.
    proxy_url : str, optional
        The proxy URL for HTTP requests.
    max_requests_per_second : int, optional
        The maximum number of requests per second for rate limiting.

    Returns
    -------
    nautilus_pyo3.KrakenFuturesHttpClient

    """
    return nautilus_pyo3.KrakenFuturesHttpClient(
        api_key=api_key,
        api_secret=api_secret,
        base_url=base_url,
        demo=demo,
        timeout_secs=timeout_secs,
        max_retries=max_retries,
        retry_delay_ms=retry_delay_ms,
        retry_delay_max_ms=retry_delay_max_ms,
        proxy_url=proxy_url,
        max_requests_per_second=max_requests_per_second,
    )


@lru_cache(1)
def get_cached_kraken_instrument_provider(
    http_client_spot: nautilus_pyo3.KrakenSpotHttpClient | None,
    http_client_futures: nautilus_pyo3.KrakenFuturesHttpClient | None,
    product_types: tuple[KrakenProductType, ...],
    config: InstrumentProviderConfig,
) -> KrakenInstrumentProvider:
    """
    Cache and return a Kraken instrument provider.

    If a cached provider already exists, then that provider will be returned.

    Parameters
    ----------
    http_client_spot : nautilus_pyo3.KrakenSpotHttpClient, optional
        The Kraken Spot HTTP client.
    http_client_futures : nautilus_pyo3.KrakenFuturesHttpClient, optional
        The Kraken Futures HTTP client.
    product_types : tuple[KrakenProductType, ...]
        The product types to load.
    config : InstrumentProviderConfig
        The instrument provider configuration.

    Returns
    -------
    KrakenInstrumentProvider

    """
    return KrakenInstrumentProvider(
        http_client_spot=http_client_spot,
        http_client_futures=http_client_futures,
        product_types=list(product_types),
        config=config,
    )


class KrakenLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Kraken live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str | None,
        config: KrakenDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> KrakenDataClient:
        """
        Create a new Kraken data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str, optional
            The custom client ID.
        config : KrakenDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        KrakenDataClient

        """
        environment = config.environment or KrakenEnvironment.MAINNET
        product_types = list(config.product_types or (KrakenProductType.SPOT,))
        is_demo = environment == KrakenEnvironment.DEMO

        # Get cached HTTP clients for each requested product type
        http_client_spot: nautilus_pyo3.KrakenSpotHttpClient | None = None
        http_client_futures: nautilus_pyo3.KrakenFuturesHttpClient | None = None

        if KrakenProductType.SPOT in product_types:
            http_client_spot = get_cached_kraken_spot_http_client(
                api_key=config.api_key,
                api_secret=config.api_secret,
                base_url=config.base_url_http_spot,
                demo=is_demo,
                timeout_secs=config.http_timeout_secs,
                max_retries=config.max_retries,
                retry_delay_ms=config.retry_delay_initial_ms,
                retry_delay_max_ms=config.retry_delay_max_ms,
                proxy_url=config.http_proxy_url,
                max_requests_per_second=config.max_requests_per_second,
            )

        if KrakenProductType.FUTURES in product_types:
            http_client_futures = get_cached_kraken_futures_http_client(
                api_key=config.api_key,
                api_secret=config.api_secret,
                base_url=config.base_url_http_futures,
                demo=is_demo,
                timeout_secs=config.http_timeout_secs,
                max_retries=config.max_retries,
                retry_delay_ms=config.retry_delay_initial_ms,
                retry_delay_max_ms=config.retry_delay_max_ms,
                proxy_url=config.http_proxy_url,
                max_requests_per_second=config.max_requests_per_second,
            )

        provider = get_cached_kraken_instrument_provider(
            http_client_spot=http_client_spot,
            http_client_futures=http_client_futures,
            product_types=tuple(product_types),
            config=config.instrument_provider,
        )

        return KrakenDataClient(
            loop=loop,
            http_client_spot=http_client_spot,
            http_client_futures=http_client_futures,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class KrakenLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a Kraken live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str | None,
        config: KrakenExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> KrakenExecutionClient:
        """
        Create a new Kraken execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str, optional
            The custom client ID.
        config : KrakenExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        KrakenExecutionClient

        """
        environment = config.environment or KrakenEnvironment.MAINNET
        product_types = list(config.product_types or (KrakenProductType.SPOT,))
        is_demo = environment == KrakenEnvironment.DEMO

        # Get cached HTTP clients for each requested product type
        http_client_spot: nautilus_pyo3.KrakenSpotHttpClient | None = None
        http_client_futures: nautilus_pyo3.KrakenFuturesHttpClient | None = None

        if KrakenProductType.SPOT in product_types:
            http_client_spot = get_cached_kraken_spot_http_client(
                api_key=config.api_key,
                api_secret=config.api_secret,
                base_url=config.base_url_http_spot,
                demo=is_demo,
                timeout_secs=config.http_timeout_secs,
                max_retries=config.max_retries,
                retry_delay_ms=config.retry_delay_initial_ms,
                retry_delay_max_ms=config.retry_delay_max_ms,
                proxy_url=config.http_proxy_url,
                max_requests_per_second=config.max_requests_per_second,
            )

        if KrakenProductType.FUTURES in product_types:
            http_client_futures = get_cached_kraken_futures_http_client(
                api_key=config.api_key,
                api_secret=config.api_secret,
                base_url=config.base_url_http_futures,
                demo=is_demo,
                timeout_secs=config.http_timeout_secs,
                max_retries=config.max_retries,
                retry_delay_ms=config.retry_delay_initial_ms,
                retry_delay_max_ms=config.retry_delay_max_ms,
                proxy_url=config.http_proxy_url,
                max_requests_per_second=config.max_requests_per_second,
            )

        provider = get_cached_kraken_instrument_provider(
            http_client_spot=http_client_spot,
            http_client_futures=http_client_futures,
            product_types=tuple(product_types),
            config=config.instrument_provider,
        )

        return KrakenExecutionClient(
            loop=loop,
            http_client_spot=http_client_spot,
            http_client_futures=http_client_futures,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
