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

from functools import lru_cache
from typing import TYPE_CHECKING

from nautilus_trader.adapters.bybit.common.constants import BYBIT_ALL_PRODUCTS
from nautilus_trader.adapters.bybit.common.credentials import get_api_key
from nautilus_trader.adapters.bybit.common.credentials import get_api_secret
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.urls import get_http_base_url
from nautilus_trader.adapters.bybit.common.urls import get_ws_base_url_private
from nautilus_trader.adapters.bybit.common.urls import get_ws_base_url_public
from nautilus_trader.adapters.bybit.common.urls import get_ws_base_url_trade
from nautilus_trader.adapters.bybit.data import BybitDataClient
from nautilus_trader.adapters.bybit.execution import BybitExecutionClient
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.providers import BybitInstrumentProvider
from nautilus_trader.core.nautilus_pyo3 import Quota
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


if TYPE_CHECKING:
    import asyncio

    from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
    from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
    from nautilus_trader.cache.cache import Cache
    from nautilus_trader.common.component import LiveClock
    from nautilus_trader.common.component import MessageBus
    from nautilus_trader.config import InstrumentProviderConfig


@lru_cache(1)
def get_bybit_http_client(
    clock: LiveClock,
    key: str | None = None,
    secret: str | None = None,
    base_url: str | None = None,
    is_demo: bool = False,
    is_testnet: bool = False,
    recv_window_ms: int = 5_000,
) -> BybitHttpClient:
    """
    Cache and return a Bybit HTTP client with the given key and secret.

    If a cached client with matching parameters already exists, the cached client will be returned.

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
    is_demo : bool, default False
        If the client is connecting to the demo API.
    is_testnet : bool, default False
        If the client is connecting to the testnet API.
    recv_window_ms : PositiveInt, default 5000
        The receive window (milliseconds) for Bybit HTTP requests.

    Returns
    -------
    BybitHttpClient

    """
    key = key or get_api_key(is_demo, is_testnet)
    secret = secret or get_api_secret(is_demo, is_testnet)
    http_base_url = base_url or get_http_base_url(is_demo, is_testnet)

    # Set up rate limit quotas
    # Current rate limit in bybit is 120 requests in any 5-second window,
    # and that is 24 request per second.
    # https://bybit-exchange.github.io/docs/v5/rate-limit
    ratelimiter_default_quota = Quota.rate_per_second(24)
    ratelimiter_quotas: list[tuple[str, Quota]] = []

    return BybitHttpClient(
        clock=clock,
        api_key=key,
        api_secret=secret,
        base_url=http_base_url,
        recv_window_ms=recv_window_ms,
        ratelimiter_quotas=ratelimiter_quotas,
        ratelimiter_default_quota=ratelimiter_default_quota,
    )


@lru_cache(1)
def get_bybit_instrument_provider(
    client: BybitHttpClient,
    clock: LiveClock,
    product_types: frozenset[BybitProductType],
    config: InstrumentProviderConfig,
) -> BybitInstrumentProvider:
    """
    Cache and return a Bybit instrument provider.

    If a cached provider already exists, then that cached provider will be returned.

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
        product_types=list(product_types),
    )


class BybitLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a Bybit live data client factory.
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
            is_demo=config.demo,
            is_testnet=config.testnet,
            recv_window_ms=config.recv_window_ms,
        )
        provider = get_bybit_instrument_provider(
            client=client,
            clock=clock,
            product_types=frozenset(product_types),
            config=config.instrument_provider,
        )
        ws_base_urls: dict[BybitProductType, str] = {}
        for product_type in product_types:
            ws_base_urls[product_type] = get_ws_base_url_public(
                product_type=product_type,
                is_demo=config.demo,
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
    Provides a Bybit live execution client factory.
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
            is_demo=config.demo,
            is_testnet=config.testnet,
            recv_window_ms=config.recv_window_ms,
        )
        provider = get_bybit_instrument_provider(
            client=client,
            clock=clock,
            product_types=frozenset(config.product_types or BYBIT_ALL_PRODUCTS),
            config=config.instrument_provider,
        )

        base_url_ws_private: str = get_ws_base_url_private(config.testnet)
        base_url_ws_trade: str = get_ws_base_url_trade(config.testnet)

        return BybitExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            product_types=config.product_types or [BybitProductType.SPOT],
            base_url_ws_private=config.base_url_ws_private or base_url_ws_private,
            base_url_ws_trade=config.base_url_ws_trade or base_url_ws_trade,
            config=config,
            name=name,
        )
