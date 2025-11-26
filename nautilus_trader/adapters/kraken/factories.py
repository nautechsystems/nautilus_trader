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
from nautilus_trader.adapters.kraken.data import KrakenDataClient
from nautilus_trader.adapters.kraken.providers import KrakenInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.live.factories import LiveDataClientFactory


@lru_cache(maxsize=1)
def get_kraken_http_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    base_url: str | None = None,
    timeout_secs: int | None = None,
    max_retries: int | None = None,
    retry_delay_ms: int | None = None,
    retry_delay_max_ms: int | None = None,
    proxy_url: str | None = None,
) -> nautilus_pyo3.KrakenHttpClient:
    """
    Cache and return a Kraken HTTP client with the given key and secret.

    If ``api_key`` and ``api_secret`` are ``None``, then they will be sourced from the
    environment variables ``KRAKEN_API_KEY`` and ``KRAKEN_API_SECRET``.

    Parameters
    ----------
    api_key : str, optional
        The Kraken API key for the client.
    api_secret : str, optional
        The Kraken API secret for the client.
    base_url : str, optional
        The base URL for the Kraken API.
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

    Returns
    -------
    nautilus_pyo3.KrakenHttpClient

    """
    return nautilus_pyo3.KrakenHttpClient(
        api_key=api_key,
        api_secret=api_secret,
        base_url=base_url,
        timeout_secs=timeout_secs,
        max_retries=max_retries,
        retry_delay_ms=retry_delay_ms,
        retry_delay_max_ms=retry_delay_max_ms,
        proxy_url=proxy_url,
    )


@lru_cache(maxsize=1)
def get_kraken_instrument_provider(
    client: nautilus_pyo3.KrakenHttpClient,
    product_types: tuple[str, ...],
    config: InstrumentProviderConfig,
) -> KrakenInstrumentProvider:
    """
    Cache and return a Kraken instrument provider.

    Parameters
    ----------
    client : nautilus_pyo3.KrakenHttpClient
        The Kraken HTTP client.
    product_types : tuple[str, ...]
        The product types to load.
    config : InstrumentProviderConfig
        The instrument provider configuration.

    Returns
    -------
    KrakenInstrumentProvider

    """
    return KrakenInstrumentProvider(
        client=client,
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
        client = get_kraken_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            base_url=config.base_url_http,
            timeout_secs=config.http_timeout_secs,
            max_retries=config.max_retries,
            retry_delay_ms=config.retry_delay_initial_ms,
            retry_delay_max_ms=config.retry_delay_max_ms,
            proxy_url=config.http_proxy_url,
        )

        product_types = tuple(config.product_types or ["spot"])

        provider = get_kraken_instrument_provider(
            client=client,
            product_types=product_types,
            config=config.instrument_provider,
        )

        return KrakenDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
