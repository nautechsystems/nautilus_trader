# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.bybit.common.constants import BYBIT_ALL_PRODUCTS
from nautilus_trader.adapters.bybit.common.credentials import get_api_key
from nautilus_trader.adapters.bybit.common.credentials import get_api_secret
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.urls import get_http_base_url
from nautilus_trader.adapters.bybit.common.urls import get_ws_base_url_private
from nautilus_trader.adapters.bybit.common.urls import get_ws_base_url_public
from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.data import BybitDataClient
from nautilus_trader.adapters.bybit.execution import BybitExecutionClient
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.providers import BybitInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.nautilus_pyo3 import Quota
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


HTTP_CLIENTS: dict[str, BybitHttpClient] = {}


def get_bybit_http_client(
    clock: LiveClock,
    key: str | None = None,
    secret: str | None = None,
    base_url: str | None = None,
    is_testnet: bool = False,
) -> BybitHttpClient:
    """
    Cache and return a Bybit HTTP client with the given key and secret.

    If a cached client with matching key and secret already exists, then that cached
    client will be returned.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
    key : str, optional
        The API key for the client.
    secret : str, optional
        The API secret for the client.
    base_url : str, optional
        The base URL for the API endpoints.
    is_testnet : bool, default False
        If the client is connecting to the testnet API.

    Returns
    -------
    BinanceHttpClient

    """
    global HTTP_CLIENTS
    key = key or get_api_key(is_testnet)
    secret = secret or get_api_secret(is_testnet)
    http_base_url = base_url or get_http_base_url(is_testnet)
    client_key: str = "|".join((key, secret))

    # Setup rate limit quotas
    # Current rate limit in bybit is 120 requests in any 5-second window,
    # and that is 24 request per second.
    # https://bybit-exchange.github.io/docs/v5/rate-limit
    ratelimiter_default_quota = Quota.rate_per_second(24)
    ratelimiter_quotas: list[tuple[str, Quota]] = []

    if client_key not in HTTP_CLIENTS:
        client = BybitHttpClient(
            clock=clock,
            api_key=key,
            api_secret=secret,
            base_url=http_base_url,
            ratelimiter_quotas=ratelimiter_quotas,
            ratelimiter_default_quota=ratelimiter_default_quota,
        )
        HTTP_CLIENTS[client_key] = client
    return HTTP_CLIENTS[client_key]


def get_bybit_instrument_provider(
    client: BybitHttpClient,
    clock: LiveClock,
    product_types: list[BybitProductType],
    config: InstrumentProviderConfig,
) -> BybitInstrumentProvider:
    """
    Cache and return a Bybit instrument provider.

    If a cached provider with matching key and secret already exists, then that
    cached provider will be returned.

    Parameters
    ----------
    client : BybitHttpClient
        The client for the instrument provider.
    clock : LiveClock
        The clock for the instrument provider.
    product_types : list[BybitProductType]
        The product types to load.
    is_testnet : bool
        If the provider is for the Spot testnet.
    config : InstrumentProviderConfig
        The configuration for the instrument provider.

    Returns
    -------
    BybitInstrumentProvider

    """
    return BybitInstrumentProvider(
        client=client,
        config=config,
        clock=clock,
        product_types=product_types,
    )


class BybitLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a `Bybit` live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BybitDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> BybitDataClient:
        """
        Create a new Bybit data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : BybitDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock: LiveClock
            The clock for the instrument provider.

        Returns
        -------
        BybitDataClient

        """
        product_types = config.product_types or BYBIT_ALL_PRODUCTS
        client: BybitHttpClient = get_bybit_http_client(
            clock=clock,
            key=config.api_key,
            secret=config.api_secret,
            base_url=config.base_url_http,
            is_testnet=config.testnet,
        )
        provider = get_bybit_instrument_provider(
            client=client,
            clock=clock,
            product_types=product_types,
            config=config.instrument_provider,
        )
        ws_base_urls: dict[BybitProductType, str] = {}
        for product_type in product_types:
            ws_base_urls[product_type] = get_ws_base_url_public(
                product_type=product_type,
                is_testnet=config.testnet,
            )
        return BybitDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            product_types=product_types,
            ws_base_urls=ws_base_urls,
            config=config,
            name=name,
        )


class BybitLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a `Bybit` live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: BybitExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> BybitExecutionClient:
        """
        Create a new Bybit execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : BybitExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        BybitExecutionClient

        """
        client: BybitHttpClient = get_bybit_http_client(
            clock=clock,
            key=config.api_key,
            secret=config.api_secret,
            base_url=config.base_url_http,
            is_testnet=config.testnet,
        )
        provider = get_bybit_instrument_provider(
            client=client,
            clock=clock,
            product_types=config.product_types or BYBIT_ALL_PRODUCTS,
            config=config.instrument_provider,
        )
        base_url_ws: str = get_ws_base_url_private(config.testnet)
        return BybitExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            product_types=config.product_types or [BybitProductType.SPOT],
            base_url_ws=config.base_url_ws or base_url_ws,
            config=config,
            name=name,
        )
