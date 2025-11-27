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

from nautilus_trader.adapters.kraken.config import KrakenDataClientConfig
from nautilus_trader.adapters.kraken.data import KrakenDataClient
from nautilus_trader.adapters.kraken.providers import KrakenInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import KrakenEnvironment
from nautilus_trader.core.nautilus_pyo3 import KrakenProductType
from nautilus_trader.live.factories import LiveDataClientFactory


def get_kraken_http_client(
    product_type: KrakenProductType,
    api_key: str | None = None,
    api_secret: str | None = None,
    base_url: str | None = None,
    testnet: bool = False,
    timeout_secs: int | None = None,
    max_retries: int | None = None,
    retry_delay_ms: int | None = None,
    retry_delay_max_ms: int | None = None,
    proxy_url: str | None = None,
) -> nautilus_pyo3.KrakenHttpClient:
    """
    Return a Kraken HTTP client for the given product type.

    If ``api_key`` and ``api_secret`` are ``None``, then they will be sourced from the
    environment variables ``KRAKEN_API_KEY`` and ``KRAKEN_API_SECRET`` (or
    ``KRAKEN_TESTNET_API_KEY`` and ``KRAKEN_TESTNET_API_SECRET`` if testnet is True).

    Parameters
    ----------
    product_type : KrakenProductType
        The Kraken product type (SPOT or FUTURES).
    api_key : str, optional
        The Kraken API key for the client.
    api_secret : str, optional
        The Kraken API secret for the client.
    base_url : str, optional
        The base URL for the Kraken API.
    testnet : bool, default False
        If True, use testnet environment variables for credentials.
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
        product_type=product_type,
        api_key=api_key,
        api_secret=api_secret,
        base_url=base_url,
        testnet=testnet,
        timeout_secs=timeout_secs,
        max_retries=max_retries,
        retry_delay_ms=retry_delay_ms,
        retry_delay_max_ms=retry_delay_max_ms,
        proxy_url=proxy_url,
    )


def get_kraken_instrument_provider(
    http_clients: dict[KrakenProductType, nautilus_pyo3.KrakenHttpClient],
    product_types: list[KrakenProductType],
    config: InstrumentProviderConfig,
) -> KrakenInstrumentProvider:
    """
    Return a Kraken instrument provider.

    Parameters
    ----------
    http_clients : dict[KrakenProductType, nautilus_pyo3.KrakenHttpClient]
        The Kraken HTTP clients keyed by product type.
    product_types : list[KrakenProductType]
        The product types to load.
    config : InstrumentProviderConfig
        The instrument provider configuration.

    Returns
    -------
    KrakenInstrumentProvider

    """
    return KrakenInstrumentProvider(
        http_clients=http_clients,
        product_types=product_types,
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
        is_testnet = environment == KrakenEnvironment.TESTNET

        # Create HTTP clients for each product type
        http_clients: dict[KrakenProductType, nautilus_pyo3.KrakenHttpClient] = {}

        for product_type in product_types:
            # Honor config override, fall back to derived URL if not specified
            base_url = config.base_url_http or nautilus_pyo3.get_kraken_http_base_url(
                product_type,
                environment,
            )

            client = get_kraken_http_client(
                product_type=product_type,
                api_key=config.api_key,
                api_secret=config.api_secret,
                base_url=base_url,
                testnet=is_testnet,
                timeout_secs=config.http_timeout_secs,
                max_retries=config.max_retries,
                retry_delay_ms=config.retry_delay_initial_ms,
                retry_delay_max_ms=config.retry_delay_max_ms,
                proxy_url=config.http_proxy_url,
            )
            http_clients[product_type] = client

        provider = get_kraken_instrument_provider(
            http_clients=http_clients,
            product_types=product_types,
            config=config.instrument_provider,
        )

        return KrakenDataClient(
            loop=loop,
            http_clients=http_clients,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
