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

from nautilus_trader.adapters.bitmex.config import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex.data import BitmexDataClient
from nautilus_trader.adapters.bitmex.execution import BitmexExecClientConfig
from nautilus_trader.adapters.bitmex.execution import BitmexExecutionClient
from nautilus_trader.adapters.bitmex.providers import BitmexInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


@lru_cache(maxsize=1)
def get_bitmex_http_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    base_url: str | None = None,
    testnet: bool = False,
    timeout_secs: int | None = None,
    max_retries: int | None = None,
    retry_delay_ms: int | None = None,
    retry_delay_max_ms: int | None = None,
    recv_window_ms: int | None = None,
    max_requests_per_second: int | None = None,
    max_requests_per_minute: int | None = None,
) -> nautilus_pyo3.BitmexHttpClient:
    """
    Cache and return a BitMEX HTTP client with the given key and secret.

    If ``api_key`` and ``api_secret`` are ``None``, then they will be sourced from the
    environment variables ``BITMEX_API_KEY`` and ``BITMEX_API_SECRET`` for production,
    or ``BITMEX_TESTNET_API_KEY`` and ``BITMEX_TESTNET_API_SECRET`` when ``testnet=True``.

    Parameters
    ----------
    api_key : str, optional
        The BitMEX API key for the client.
    api_secret : str, optional
        The BitMEX API secret for the client.
    base_url : str, optional
        The base URL for the BitMEX API.
    testnet : bool, default False
        If the client should connect to the testnet.
    timeout_secs : int, optional
        The timeout in seconds for HTTP requests.
    max_retries : int, optional
        The maximum number of retry attempts for failed requests.
    retry_delay_ms : int, optional
        The initial delay in milliseconds between retry attempts.
    retry_delay_max_ms : int, optional
        The maximum delay in milliseconds between retry attempts.
    recv_window_ms : int, optional
        The expiration window in milliseconds for signed requests.
    max_requests_per_second : int, optional
        Maximum REST burst rate (requests per second).
    max_requests_per_minute : int, optional
        Maximum REST rolling rate (requests per minute).

    Returns
    -------
    nautilus_pyo3.BitmexHttpClient

    """
    return nautilus_pyo3.BitmexHttpClient(
        api_key=api_key,
        api_secret=api_secret,
        base_url=base_url,
        testnet=testnet,
        timeout_secs=timeout_secs,
        max_retries=max_retries,
        retry_delay_ms=retry_delay_ms,
        retry_delay_max_ms=retry_delay_max_ms,
        recv_window_ms=recv_window_ms,
        max_requests_per_second=max_requests_per_second,
        max_requests_per_minute=max_requests_per_minute,
    )


@lru_cache(maxsize=1)
def get_bitmex_instrument_provider(
    client: nautilus_pyo3.BitmexHttpClient,
    active_only: bool,
    config: InstrumentProviderConfig,
) -> BitmexInstrumentProvider:
    """
    Cache and return a BitMEX instrument provider.

    Parameters
    ----------
    client : nautilus_pyo3.BitmexHttpClient
        The BitMEX HTTP client.
    active_only : bool
        Whether to only load active instruments.
    config : InstrumentProviderConfig
        The instrument provider configuration.

    Returns
    -------
    BitmexInstrumentProvider

    """
    return BitmexInstrumentProvider(
        client=client,
        active_only=active_only,
        config=config,
    )


class BitmexLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a BitMEX live data client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str | None,
        config: BitmexDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> BitmexDataClient:
        """
        Create a new BitMEX data client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str, optional
            The custom client ID.
        config : BitmexDataClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        BitmexDataClient

        """
        client = get_bitmex_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            base_url=config.base_url_http,
            testnet=config.testnet,
            timeout_secs=config.http_timeout_secs,
            max_retries=config.max_retries,
            retry_delay_ms=config.retry_delay_initial_ms,
            retry_delay_max_ms=config.retry_delay_max_ms,
            recv_window_ms=config.recv_window_ms,
            max_requests_per_second=config.max_requests_per_second,
            max_requests_per_minute=config.max_requests_per_minute,
        )

        provider = get_bitmex_instrument_provider(
            client=client,
            active_only=True,  # Always use active instruments for live clients
            config=config.instrument_provider,
        )

        return BitmexDataClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )


class BitmexLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a BitMEX live execution client factory.
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str | None,
        config: BitmexExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> BitmexExecutionClient:
        """
        Create a new BitMEX execution client.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str, optional
            The custom client ID.
        config : BitmexExecClientConfig
            The client configuration.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        BitmexExecutionClient

        """
        client = get_bitmex_http_client(
            api_key=config.api_key,
            api_secret=config.api_secret,
            base_url=config.base_url_http,
            testnet=config.testnet,
            timeout_secs=config.http_timeout_secs,
            max_retries=config.max_retries,
            retry_delay_ms=config.retry_delay_initial_ms,
            retry_delay_max_ms=config.retry_delay_max_ms,
            recv_window_ms=config.recv_window_ms,
            max_requests_per_second=config.max_requests_per_second,
            max_requests_per_minute=config.max_requests_per_minute,
        )

        provider = get_bitmex_instrument_provider(
            client=client,
            active_only=True,  # Always use active instruments for live clients
            config=config.instrument_provider,
        )

        return BitmexExecutionClient(
            loop=loop,
            client=client,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
            name=name,
        )
